use crate::commands::types::{format_wei, hex_encode, parse_token_amount, TransactionRequest};
use crate::gossip_actor::GossipCmd;
use crate::state::AppState;
use alloy::primitives::{Address, FixedBytes, TxHash};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::sol_types::SolEvent;
use ara_chain::contracts::IContentRegistry;
use ara_chain::registry::RegistryClient;
use ara_p2p::content::ContentManager;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tauri::State;
use tracing::info;

#[derive(Serialize, Deserialize)]
pub struct ContentDetail {
    pub content_id: String,
    pub content_hash: String,
    pub creator: String,
    pub title: String,
    pub description: String,
    pub content_type: String,
    pub price_eth: String,
    pub active: bool,
    pub seeder_count: u32,
    pub purchase_count: u32,
}

#[derive(Serialize, Deserialize)]
pub struct PublishPrepareResult {
    /// BLAKE3 content hash (0x-prefixed hex)
    pub content_hash: String,
    /// Metadata URI stored locally
    pub metadata_uri: String,
    /// Transactions to sign (empty if registry not deployed)
    pub transactions: Vec<TransactionRequest>,
}

#[tauri::command]
pub async fn publish_content(
    state: State<'_, AppState>,
    file_path: String,
    title: String,
    _description: String,
    _content_type: String,
    price_eth: String,
) -> Result<PublishPrepareResult, String> {
    info!(
        "Publishing content: title={}, file={}, price={}",
        title, file_path, price_eth
    );

    // 1. Require wallet connected
    let wallet = state.wallet_address.lock().await;
    let creator = wallet.as_ref().ok_or("No wallet connected")?.clone();
    drop(wallet);

    // 2. Parse price
    let price_wei = parse_token_amount(&price_eth)?;

    // 3. Start iroh node (lazy), extract BlobsClient + NodeId + relay URL,
    //    then drop the guard before doing async work
    let (blobs_client, node_id_str, relay_url_str) = {
        let guard = state.ensure_iroh().await?;
        let node = guard.as_ref().unwrap();
        let relay_url = node
            .node_addr()
            .await
            .ok()
            .and_then(|addr| addr.relay_url().map(|u| u.to_string()))
            .unwrap_or_default();
        (node.blobs_client(), node.node_id().to_string(), relay_url)
    };

    let file = Path::new(&file_path);
    if !file.exists() {
        return Err(format!("File not found: {}", file_path));
    }

    let content_mgr = ContentManager::new(blobs_client);
    let content_hash_bytes = content_mgr
        .import_file(file)
        .await
        .map_err(|e| format!("Failed to import file: {e}"))?;

    let content_hash_hex = format!("0x{}", alloy::hex::encode(content_hash_bytes));
    info!("File imported, BLAKE3 hash: {}", content_hash_hex);

    // 4. Build metadata URI as JSON (discoverable by other nodes via on-chain events)
    let metadata_uri = {
        let meta = serde_json::json!({
            "v": 1,
            "title": &title,
            "description": &_description,
            "content_type": &_content_type,
            "filename": &Path::new(&file_path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            "file_size": std::fs::metadata(&file_path).map(|m| m.len()).unwrap_or(0),
            "node_id": &node_id_str,
            "relay_url": &relay_url_str,
        });
        serde_json::to_string(&meta).map_err(|e| format!("Failed to serialize metadata: {e}"))?
    };

    // 5. Get file size and original filename
    let file_size = std::fs::metadata(&file_path)
        .map(|m| m.len() as i64)
        .unwrap_or(0);
    let filename = Path::new(&file_path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| format!("{}.bin", alloy::hex::encode(&content_hash_bytes[..8])));

    // 6. Store metadata in SQLite (active = 0 until confirmed).
    //    Use INSERT (not INSERT OR REPLACE) so previous confirmed rows are untouched.
    //    The temp content_id is the blake3 hash; confirm_publish replaces it with the
    //    on-chain contentId from the ContentPublished event.
    let created_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    {
        let db = state.db.lock().await;
        // Delete any previous unconfirmed row for this hash (stale publish attempt)
        db.conn()
            .execute(
                "DELETE FROM content WHERE content_hash = ?1 AND active = 0",
                rusqlite::params![&content_hash_hex],
            )
            .ok();
        db.conn()
            .execute(
                "INSERT INTO content
                 (content_id, content_hash, creator, metadata_uri, price_wei,
                  title, description, content_type, file_size_bytes, active, created_at,
                  filename, publisher_node_id, publisher_relay_url)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, ?10, ?11, ?12, ?13)",
                rusqlite::params![
                    &content_hash_hex,
                    &content_hash_hex,
                    &creator,
                    &metadata_uri,
                    price_wei.to_string(),
                    &title,
                    &_description,
                    &_content_type,
                    file_size,
                    created_at,
                    &filename,
                    &node_id_str,
                    &relay_url_str,
                ],
            )
            .map_err(|e| format!("DB insert failed: {e}"))?;
    }

    info!("Metadata stored in DB for {}", content_hash_hex);

    // 7. Build on-chain publishContent calldata
    let registry_addr_str = &state.config.ethereum.registry_address;
    let registry_addr = if registry_addr_str.is_empty() {
        Address::ZERO
    } else {
        registry_addr_str
            .parse::<Address>()
            .map_err(|e| format!("Invalid registry address: {e}"))?
    };

    let transactions = if registry_addr == Address::ZERO {
        info!("Registry not deployed — local-only publish");
        vec![]
    } else {
        let content_hash_fixed = FixedBytes::from(content_hash_bytes);

        // Pre-flight: check publisher eligibility (ARA stake).
        // Returns a clear error before MetaMask opens. Never skips MetaMask.
        if let Ok(chain) = state.chain_client() {
            let creator_addr: Address = creator
                .parse()
                .map_err(|e| format!("Invalid creator address: {e}"))?;

            match chain.staking.is_eligible_publisher(creator_addr).await {
                Ok(false) => {
                    return Err(
                        "Insufficient ARA stake to publish. Stake more ARA in the Dashboard first."
                            .to_string(),
                    );
                }
                Err(e) => {
                    info!("Could not check publisher eligibility (RPC error): {e}");
                }
                Ok(true) => {}
            }
        }

        let calldata = RegistryClient::<()>::publish_content_calldata(
            content_hash_fixed,
            metadata_uri.clone(),
            price_wei,
        );

        vec![TransactionRequest {
            to: format!("{registry_addr:#x}"),
            data: hex_encode(&calldata),
            value: "0x0".to_string(),
            description: format!("Publish \"{}\" for {} ETH", title, price_eth),
        }]
    };

    Ok(PublishPrepareResult {
        content_hash: content_hash_hex,
        metadata_uri,
        transactions,
    })
}

/// Called by frontend after the publish transaction is confirmed (or immediately for local-only).
/// Extracts contentId from the on-chain ContentPublished event, marks content as active,
/// auto-starts seeding, and announces on gossip.
#[tauri::command]
pub async fn confirm_publish(
    state: State<'_, AppState>,
    content_hash: String,
    tx_hash: String,
) -> Result<(), String> {
    info!(
        "Confirming publish: hash={}, tx={}",
        content_hash, tx_hash
    );

    // Get publisher's node_id and relay URL from iroh
    let (node_id_str, relay_url_str) = {
        let guard = state.ensure_iroh().await?;
        let node = guard.as_ref().unwrap();
        let relay_url = node
            .node_addr()
            .await
            .ok()
            .and_then(|addr| addr.relay_url().map(|u| u.to_string()))
            .unwrap_or_default();
        (node.node_id().to_string(), relay_url)
    };

    let content_hash_bytes = parse_content_hash_bytes(&content_hash)?;

    // Extract contentId: from on-chain event if real tx, or use blake3 hash for local-only
    let content_id_hex = if tx_hash != "0x0" {
        match extract_content_id_from_receipt(&state, &tx_hash).await {
            Ok(id) => id,
            Err(receipt_err) => {
                // RPC may be down or tx not yet indexed. Check if the periodic sync already
                // populated the content row (it runs every 30s in the background).
                info!("Receipt extraction failed ({receipt_err}), checking DB for synced content_id");
                let synced_id: Option<String> = {
                    let db = state.db.lock().await;
                    db.conn()
                        .query_row(
                            "SELECT content_id FROM content WHERE content_hash = ?1 AND active = 1",
                            rusqlite::params![&content_hash],
                            |row| row.get(0),
                        )
                        .ok()
                };
                match synced_id {
                    Some(id) => {
                        info!("Using synced content_id from DB: {}", id);
                        id
                    }
                    None => return Err(format!("Failed to confirm publish: {receipt_err}")),
                }
            }
        }
    } else {
        // Local-only mode: use blake3 hash as content_id
        content_hash.clone()
    };

    info!("On-chain contentId: {}", content_id_hex);

    // Mark content as active, update content_id to on-chain value, store publisher's node_id
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    {
        let db = state.db.lock().await;
        let conn = db.conn();

        // Update the pending row (active=0) for this content_hash to the on-chain contentId.
        // If sync already ran and created the active=1 row with this content_id, the UPDATE
        // will hit a UNIQUE constraint. In that case, just delete the now-redundant temp row.
        match conn.execute(
            "UPDATE content SET content_id = ?1, active = 1, publisher_node_id = ?2, publisher_relay_url = ?3
             WHERE content_hash = ?4 AND active = 0",
            rusqlite::params![&content_id_hex, &node_id_str, &relay_url_str, &content_hash],
        ) {
            Ok(_) => {} // Updated temp row, or no active=0 row existed — both fine
            Err(e) if e.to_string().contains("UNIQUE") => {
                // Sync already inserted the active=1 row with content_id=keccak256.
                // Delete the stale temp row (blake3 content_id, active=0).
                conn.execute(
                    "DELETE FROM content WHERE content_hash = ?1 AND active = 0",
                    rusqlite::params![&content_hash],
                )
                .ok();
                info!("Cleaned up temp content row (sync had already created active row)");
            }
            Err(e) => return Err(format!("DB update failed: {e}")),
        }

        // Auto-start seeding (publisher always seeds their own content).
        // INSERT OR IGNORE preserves existing bytes_served if this is called a second time.
        conn.execute(
            "INSERT OR IGNORE INTO seeding (content_id, active, bytes_served, peer_count, started_at)
             VALUES (?1, 1, 0, 0, ?2)",
            rusqlite::params![&content_id_hex, now],
        )
        .map_err(|e| format!("Seeding DB insert failed: {e}"))?;
        // Ensure seeding is marked active (in case it was previously stopped)
        conn.execute(
            "UPDATE seeding SET active = 1 WHERE content_id = ?1",
            rusqlite::params![&content_id_hex],
        )
        .ok();
    }

    // Announce seeding on gossip (publisher is first seeder — no bootstrap peers)
    state
        .send_gossip(GossipCmd::AnnounceSeeding {
            content_hash: content_hash_bytes,
            bootstrap: vec![],
        })
        .await?;

    info!(
        "Content {} (contentId={}) is now active, seeding, and announced on gossip",
        content_hash, content_id_hex
    );
    Ok(())
}

/// Fetch the transaction receipt and extract the contentId from the ContentPublished event.
async fn extract_content_id_from_receipt(
    state: &AppState,
    tx_hash: &str,
) -> Result<String, String> {
    let hash: TxHash = tx_hash
        .parse()
        .map_err(|e| format!("Invalid tx hash: {e}"))?;

    // Try the primary RPC URL
    let rpc_url = &state.config.ethereum.rpc_url;
    let parsed_url = rpc_url
        .parse()
        .map_err(|e| format!("Invalid RPC URL: {e}"))?;
    let provider = ProviderBuilder::new().connect_http(parsed_url);

    let receipt = provider
        .get_transaction_receipt(hash)
        .await
        .map_err(|e| format!("Failed to fetch tx receipt: {e}"))?
        .ok_or_else(|| format!("Transaction receipt not found for {tx_hash}"))?;

    // Parse logs for ContentPublished event
    for log in receipt.inner.logs() {
        if let Ok(event) = IContentRegistry::ContentPublished::decode_log(&log.inner) {
            let content_id = format!("0x{}", alloy::hex::encode(event.contentId.as_slice()));
            info!("Extracted contentId from event: {}", content_id);
            return Ok(content_id);
        }
    }

    Err(format!(
        "ContentPublished event not found in receipt for tx {tx_hash}"
    ))
}

/// Prepare an on-chain updateContent transaction to change metadata and/or price.
/// The connected wallet must be the content creator.
#[tauri::command]
pub async fn update_content(
    state: State<'_, AppState>,
    content_id: String,
    title: String,
    description: String,
    content_type: String,
    price_eth: String,
) -> Result<Vec<TransactionRequest>, String> {
    info!(
        "Updating content: id={}, title={}, price={}",
        content_id, title, price_eth
    );

    // 1. Require wallet connected
    let wallet = state.wallet_address.lock().await;
    let caller = wallet.as_ref().ok_or("No wallet connected")?.clone();
    drop(wallet);

    // 2. Verify ownership from local DB and fetch preserved fields
    let (old_filename, old_file_size, old_node_id, old_relay_url) = {
        let db = state.db.lock().await;
        let conn = db.conn();
        conn.query_row(
            "SELECT creator, COALESCE(filename,''), COALESCE(file_size_bytes,0), COALESCE(publisher_node_id,''), COALESCE(publisher_relay_url,'') FROM content WHERE content_id = ?1",
            rusqlite::params![&content_id],
            |row| {
                let creator: String = row.get(0)?;
                Ok((creator, row.get::<_, String>(1)?, row.get::<_, i64>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?))
            },
        )
        .map(|(creator, filename, file_size, node_id, relay_url)| {
            if creator.to_lowercase() != caller.to_lowercase() {
                Err(format!("Only the creator ({}) can edit this content", creator))
            } else {
                Ok((filename, file_size, node_id, relay_url))
            }
        })
        .map_err(|e| format!("Content not found: {e}"))?
    }?;

    // 3. Parse price
    let price_wei = parse_token_amount(&price_eth)?;

    // 4. Build new metadata URI JSON
    let metadata_uri = {
        let meta = serde_json::json!({
            "v": 1,
            "title": &title,
            "description": &description,
            "content_type": &content_type,
            "filename": &old_filename,
            "file_size": old_file_size,
            "node_id": &old_node_id,
            "relay_url": &old_relay_url,
        });
        serde_json::to_string(&meta).map_err(|e| format!("Failed to serialize metadata: {e}"))?
    };

    // 5. Build on-chain updateContent calldata
    let registry_addr_str = &state.config.ethereum.registry_address;
    let registry_addr: Address = registry_addr_str
        .parse()
        .map_err(|e| format!("Invalid registry address: {e}"))?;

    let content_id_bytes = parse_content_hash_bytes(&content_id)?;
    let content_id_fixed = FixedBytes::from(content_id_bytes);

    let calldata = RegistryClient::<()>::update_content_calldata(
        content_id_fixed,
        price_wei,
        metadata_uri,
    );

    Ok(vec![TransactionRequest {
        to: format!("{registry_addr:#x}"),
        data: hex_encode(&calldata),
        value: "0x0".to_string(),
        description: format!("Update \"{}\" to {} ETH", title, price_eth),
    }])
}

/// Called by frontend after the updateContent transaction is confirmed.
/// Updates the local DB with the new metadata.
#[tauri::command]
pub async fn confirm_update_content(
    state: State<'_, AppState>,
    content_id: String,
    title: String,
    description: String,
    content_type: String,
    price_eth: String,
) -> Result<(), String> {
    info!("Confirming content update: id={}", content_id);

    let price_wei = parse_token_amount(&price_eth)?;

    // Build the new metadata_uri JSON (preserve filename/file_size/node_id from DB)
    let db = state.db.lock().await;
    let conn = db.conn();
    let (filename, file_size, node_id, relay_url): (String, i64, String, String) = conn
        .query_row(
            "SELECT COALESCE(filename,''), COALESCE(file_size_bytes,0), COALESCE(publisher_node_id,''), COALESCE(publisher_relay_url,'') FROM content WHERE content_id = ?1",
            rusqlite::params![&content_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(|e| format!("Content not found: {e}"))?;

    let metadata_uri = {
        let meta = serde_json::json!({
            "v": 1,
            "title": &title,
            "description": &description,
            "content_type": &content_type,
            "filename": &filename,
            "file_size": file_size,
            "node_id": &node_id,
            "relay_url": &relay_url,
        });
        serde_json::to_string(&meta).map_err(|e| format!("Failed to serialize metadata: {e}"))?
    };

    conn.execute(
        "UPDATE content SET title = ?1, description = ?2, content_type = ?3, price_wei = ?4, metadata_uri = ?5 WHERE content_id = ?6",
        rusqlite::params![&title, &description, &content_type, price_wei.to_string(), &metadata_uri, &content_id],
    )
    .map_err(|e| format!("DB update failed: {e}"))?;

    info!("Content {} updated successfully", content_id);
    Ok(())
}

/// Return all content published by the connected wallet (for the "My Content" panel).
#[tauri::command]
pub async fn get_my_content(state: State<'_, AppState>) -> Result<Vec<ContentDetail>, String> {
    let wallet = state.wallet_address.lock().await;
    let creator = wallet.as_ref().ok_or("No wallet connected")?.to_lowercase();
    drop(wallet);

    let db = state.db.lock().await;
    let conn = db.conn();

    let mut stmt = conn
        .prepare(
            "SELECT content_id, content_hash, creator, title, description,
                    content_type, price_wei, active, file_size_bytes
             FROM content WHERE LOWER(creator) = ?1
             ORDER BY created_at DESC",
        )
        .map_err(|e| format!("DB query failed: {e}"))?;

    let rows = stmt
        .query_map(rusqlite::params![&creator], |row| {
            let price_wei_str: String = row.get(6)?;
            let price_wei = price_wei_str
                .parse::<alloy::primitives::U256>()
                .unwrap_or(alloy::primitives::U256::ZERO);

            Ok(ContentDetail {
                content_id: row.get(0)?,
                content_hash: row.get(1)?,
                creator: row.get(2)?,
                title: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                description: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                content_type: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                price_eth: format_wei(price_wei),
                active: row.get::<_, i32>(7)? != 0,
                seeder_count: 0,
                purchase_count: 0,
            })
        })
        .map_err(|e| format!("DB query failed: {e}"))?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(|e| format!("Row parse error: {e}"))?);
    }
    Ok(results)
}

/// Build a delistContent transaction for the connected wallet to sign.
#[tauri::command]
pub async fn delist_content(
    state: State<'_, AppState>,
    content_id: String,
) -> Result<Vec<TransactionRequest>, String> {
    info!("Preparing delist for content: {}", content_id);

    let wallet = state.wallet_address.lock().await;
    let caller = wallet.as_ref().ok_or("No wallet connected")?.clone();
    drop(wallet);

    // Verify ownership in local DB
    {
        let db = state.db.lock().await;
        let creator: String = db.conn()
            .query_row(
                "SELECT creator FROM content WHERE content_id = ?1",
                rusqlite::params![&content_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("Content not found: {e}"))?;
        if creator.to_lowercase() != caller.to_lowercase() {
            return Err(format!("Only the creator ({}) can delist this content", creator));
        }
    }

    let registry_addr_str = &state.config.ethereum.registry_address;
    let registry_addr: Address = if registry_addr_str.is_empty() {
        Address::ZERO
    } else {
        registry_addr_str.parse().map_err(|e| format!("Invalid registry address: {e}"))?
    };

    if registry_addr == Address::ZERO {
        return Ok(vec![]);
    }

    let content_id_bytes = parse_content_hash_bytes(&content_id)?;
    let content_id_fixed = FixedBytes::from(content_id_bytes);
    let calldata = RegistryClient::<()>::delist_content_calldata(content_id_fixed);

    Ok(vec![TransactionRequest {
        to: format!("{registry_addr:#x}"),
        data: hex_encode(&calldata),
        value: "0x0".to_string(),
        description: "Delist content from Ara Marketplace".to_string(),
    }])
}

/// Called after the delist transaction is confirmed (or immediately for local-only).
/// Marks the content inactive in the local DB and stops seeding/gossip.
#[tauri::command]
pub async fn confirm_delist(
    state: State<'_, AppState>,
    content_id: String,
) -> Result<(), String> {
    info!("Confirming delist: {}", content_id);

    let content_hash_hex: String = {
        let db = state.db.lock().await;
        db.conn()
            .query_row(
                "SELECT content_hash FROM content WHERE content_id = ?1",
                rusqlite::params![&content_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("Content not found: {e}"))?
    };

    {
        let db = state.db.lock().await;
        db.conn()
            .execute("UPDATE content SET active = 0 WHERE content_id = ?1", rusqlite::params![&content_id])
            .map_err(|e| format!("DB update failed: {e}"))?;
        db.conn()
            .execute("UPDATE seeding SET active = 0 WHERE content_id = ?1", rusqlite::params![&content_id])
            .ok();
    }

    if let Ok(content_hash) = parse_content_hash_bytes(&content_hash_hex) {
        let _ = state.send_gossip(GossipCmd::LeaveSeeding { content_hash }).await;
    }

    info!("Content {} delisted successfully", content_id);
    Ok(())
}

/// Parse a 0x-prefixed hex string into a 32-byte content hash.
fn parse_content_hash_bytes(s: &str) -> Result<[u8; 32], String> {
    let hex_str = s.strip_prefix("0x").unwrap_or(s);
    let bytes =
        alloy::hex::decode(hex_str).map_err(|e| format!("Invalid content hash: {e}"))?;
    if bytes.len() != 32 {
        return Err(format!("Content hash must be 32 bytes, got {}", bytes.len()));
    }
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&bytes);
    Ok(hash)
}

#[tauri::command]
pub async fn get_content_detail(
    state: State<'_, AppState>,
    content_id: String,
) -> Result<ContentDetail, String> {
    info!("Fetching content detail: {}", content_id);

    let db = state.db.lock().await;
    let conn = db.conn();

    let mut stmt = conn
        .prepare(
            "SELECT content_id, content_hash, creator, title, description,
                    content_type, price_wei, active, file_size_bytes
             FROM content WHERE content_id = ?1",
        )
        .map_err(|e| format!("DB query failed: {e}"))?;

    let detail = stmt
        .query_row(rusqlite::params![&content_id], |row| {
            let price_wei_str: String = row.get(6)?;
            let price_wei = price_wei_str
                .parse::<alloy::primitives::U256>()
                .unwrap_or(alloy::primitives::U256::ZERO);

            Ok(ContentDetail {
                content_id: row.get(0)?,
                content_hash: row.get(1)?,
                creator: row.get(2)?,
                title: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                description: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                content_type: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                price_eth: format_wei(price_wei),
                active: row.get::<_, i32>(7)? != 0,
                seeder_count: 0,    // TODO: query seeding table
                purchase_count: 0,  // TODO: count from purchases table
            })
        })
        .map_err(|e| format!("Content not found: {e}"))?;

    Ok(detail)
}

#[tauri::command]
pub async fn search_content(
    state: State<'_, AppState>,
    query: String,
) -> Result<Vec<ContentDetail>, String> {
    info!("Searching content: {}", query);

    let db = state.db.lock().await;
    let conn = db.conn();

    let mut stmt = conn
        .prepare(
            "SELECT content_id, content_hash, creator, title, description,
                    content_type, price_wei, active, file_size_bytes
             FROM content WHERE active = 1
             AND (title LIKE ?1 OR description LIKE ?1 OR content_type LIKE ?1)
             ORDER BY created_at DESC LIMIT 50",
        )
        .map_err(|e| format!("DB query failed: {e}"))?;

    let search_pattern = if query.is_empty() {
        "%".to_string()
    } else {
        format!("%{query}%")
    };

    let rows = stmt
        .query_map(rusqlite::params![&search_pattern], |row| {
            let price_wei_str: String = row.get(6)?;
            let price_wei = price_wei_str
                .parse::<alloy::primitives::U256>()
                .unwrap_or(alloy::primitives::U256::ZERO);

            Ok(ContentDetail {
                content_id: row.get(0)?,
                content_hash: row.get(1)?,
                creator: row.get(2)?,
                title: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                description: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                content_type: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                price_eth: format_wei(price_wei),
                active: row.get::<_, i32>(7)? != 0,
                seeder_count: 0,
                purchase_count: 0,
            })
        })
        .map_err(|e| format!("DB query failed: {e}"))?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(|e| format!("Row parse error: {e}"))?);
    }

    info!("Search returned {} results", results.len());
    Ok(results)
}
