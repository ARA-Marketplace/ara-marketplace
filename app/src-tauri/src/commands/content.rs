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

    // 3. Start iroh node (lazy), extract BlobsClient (Clone + Send + Sync),
    //    then drop the guard before doing async work
    let blobs_client = {
        let guard = state.ensure_iroh().await?;
        guard.as_ref().unwrap().blobs_client()
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

    // 4. Build metadata URI (local storage for now)
    let metadata_uri = format!("ara://local/{}", alloy::hex::encode(content_hash_bytes));

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
                  title, description, content_type, file_size_bytes, active, created_at, filename)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, ?10, ?11)",
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

    // Get publisher's node_id from iroh
    let node_id_str = {
        let guard = state.ensure_iroh().await?;
        let node = guard.as_ref().unwrap();
        node.node_id().to_string()
    };

    let content_hash_bytes = parse_content_hash_bytes(&content_hash)?;

    // Extract contentId: from on-chain event if real tx, or use blake3 hash for local-only
    let content_id_hex = if tx_hash != "0x0" {
        extract_content_id_from_receipt(&state, &tx_hash).await?
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
        conn.execute(
            "UPDATE content SET content_id = ?1, active = 1, publisher_node_id = ?2
             WHERE content_hash = ?3 AND active = 0",
            rusqlite::params![&content_id_hex, &node_id_str, &content_hash],
        )
        .map_err(|e| format!("DB update failed: {e}"))?;

        // Auto-start seeding (publisher always seeds their own content)
        conn.execute(
            "INSERT OR REPLACE INTO seeding (content_id, active, bytes_served, peer_count, started_at)
             VALUES (?1, 1, 0, 0, ?2)",
            rusqlite::params![&content_id_hex, now],
        )
        .map_err(|e| format!("Seeding DB insert failed: {e}"))?;
    }

    // Announce seeding on gossip
    state
        .send_gossip(GossipCmd::AnnounceSeeding {
            content_hash: content_hash_bytes,
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
