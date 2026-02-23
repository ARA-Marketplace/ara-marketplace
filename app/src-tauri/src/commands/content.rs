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

/// Content the connected wallet has published (creator = wallet).
/// Includes live seeding status for the Library Published tab.
#[derive(Serialize, Deserialize)]
pub struct PublishedItem {
    pub content_id: String,
    pub title: String,
    pub content_type: String,
    pub price_eth: String,
    pub is_seeding: bool,
    pub file_size_bytes: i64,
    pub updated_at: Option<i64>,
}

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
    /// Raw metadata_uri JSON string — frontend parses for v2 fields (previews, etc.)
    pub metadata_uri: String,
    pub updated_at: Option<i64>,
    /// Parsed categories list (from categories DB column, JSON array)
    pub categories: Vec<String>,
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

/// Result of prepare-phase for a file update transaction.
#[derive(Serialize, Deserialize)]
pub struct UpdateFileResult {
    /// New BLAKE3 content hash (0x-prefixed hex)
    pub new_content_hash: String,
    /// Transactions to sign (updateContentFile calldata)
    pub transactions: Vec<TransactionRequest>,
}

/// A single preview asset (image or video) stored as an iroh blob.
#[derive(Serialize, Deserialize, Clone)]
pub struct PreviewAsset {
    /// "image" or "video"
    pub asset_type: String,
    /// BLAKE3 hash (0x-prefixed hex)
    pub hash: String,
    /// Original filename
    pub filename: String,
    /// File size in bytes
    pub size: u64,
}

// ─── Metadata helpers ────────────────────────────────────────────────────────

/// Infer whether a file path is a preview "image" or "video" based on extension.
fn preview_asset_type(path: &str) -> &'static str {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase());
    match ext.as_deref() {
        Some("mp4") | Some("webm") | Some("mov") | Some("avi") | Some("mkv") => "video",
        _ => "image",
    }
}

/// Import a list of preview file paths into iroh and return PreviewAsset records.
async fn import_preview_files(
    content_mgr: &ContentManager,
    file_paths: &[String],
) -> Result<Vec<PreviewAsset>, String> {
    let mut assets = Vec::new();
    for path_str in file_paths {
        let p = Path::new(path_str);
        if !p.exists() {
            return Err(format!("Preview file not found: {}", path_str));
        }
        let hash_bytes = content_mgr
            .import_file(p)
            .await
            .map_err(|e| format!("Failed to import preview {}: {e}", path_str))?;
        let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        let filename = p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        assets.push(PreviewAsset {
            asset_type: preview_asset_type(path_str).to_string(),
            hash: format!("0x{}", alloy::hex::encode(hash_bytes)),
            filename,
            size,
        });
    }
    Ok(assets)
}

/// Build a v2 metadata JSON string from all provided fields.
fn build_metadata_v2(
    title: &str,
    description: &str,
    content_type: &str,
    filename: &str,
    file_size: u64,
    node_id: &str,
    relay_url: &str,
    categories: &[String],
    main_preview_image: Option<&PreviewAsset>,
    main_preview_trailer: Option<&PreviewAsset>,
    additional_previews: &[PreviewAsset],
) -> Result<String, String> {
    let previews_json: Vec<serde_json::Value> = additional_previews
        .iter()
        .map(|a| {
            serde_json::json!({
                "type": a.asset_type,
                "hash": a.hash,
                "filename": a.filename,
                "size": a.size,
            })
        })
        .collect();

    let mut meta = serde_json::json!({
        "v": 2,
        "title": title,
        "description": description,
        "content_type": content_type,
        "filename": filename,
        "file_size": file_size,
        "node_id": node_id,
        "relay_url": relay_url,
        "categories": categories,
        "previews": previews_json,
    });

    if let Some(img) = main_preview_image {
        meta["main_preview_image"] = serde_json::json!({
            "hash": img.hash,
            "filename": img.filename,
            "size": img.size,
        });
    }
    if let Some(vid) = main_preview_trailer {
        meta["main_preview_trailer"] = serde_json::json!({
            "hash": vid.hash,
            "filename": vid.filename,
            "size": vid.size,
        });
    }

    serde_json::to_string(&meta).map_err(|e| format!("Failed to serialize metadata: {e}"))
}

/// Rebuild metadata for a content update, preserving preview assets from existing metadata JSON.
fn rebuild_metadata_preserving_previews(
    existing_metadata_uri: &str,
    title: &str,
    description: &str,
    content_type: &str,
    filename: &str,
    file_size: i64,
    node_id: &str,
    relay_url: &str,
    categories: &[String],
) -> Result<String, String> {
    // Try to parse existing metadata to preserve preview fields
    let existing: serde_json::Value =
        serde_json::from_str(existing_metadata_uri).unwrap_or(serde_json::Value::Null);

    let mut meta = serde_json::json!({
        "v": 2,
        "title": title,
        "description": description,
        "content_type": content_type,
        "filename": filename,
        "file_size": file_size,
        "node_id": node_id,
        "relay_url": relay_url,
        "categories": categories,
    });

    // Preserve existing preview fields
    for key in &["main_preview_image", "main_preview_trailer", "previews"] {
        if let Some(val) = existing.get(key) {
            if !val.is_null() {
                meta[key] = val.clone();
            }
        }
    }
    // Default previews to empty array if not present
    if meta.get("previews").is_none() {
        meta["previews"] = serde_json::json!([]);
    }

    serde_json::to_string(&meta).map_err(|e| format!("Failed to serialize metadata: {e}"))
}

// ─── Commands ────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn publish_content(
    state: State<'_, AppState>,
    file_path: String,
    title: String,
    description: String,
    content_type: String,
    price_eth: String,
    categories: Option<Vec<String>>,
    main_preview_image_path: Option<String>,
    main_preview_trailer_path: Option<String>,
    preview_paths: Option<Vec<String>>,
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

    // 3. Start iroh node (lazy), extract BlobsClient + NodeId + relay URL
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

    // Import main content file
    let content_hash_bytes = content_mgr
        .import_file(file)
        .await
        .map_err(|e| format!("Failed to import file: {e}"))?;
    let content_hash_hex = format!("0x{}", alloy::hex::encode(content_hash_bytes));
    info!("File imported, BLAKE3 hash: {}", content_hash_hex);

    // Import preview assets
    let main_img_asset = if let Some(ref path) = main_preview_image_path {
        import_preview_files(&content_mgr, &[path.clone()])
            .await?
            .into_iter()
            .next()
    } else {
        None
    };
    let main_trailer_asset = if let Some(ref path) = main_preview_trailer_path {
        import_preview_files(&content_mgr, &[path.clone()])
            .await?
            .into_iter()
            .next()
    } else {
        None
    };
    let additional_assets = if let Some(ref paths) = preview_paths {
        import_preview_files(&content_mgr, paths).await?
    } else {
        vec![]
    };

    let cats = categories.unwrap_or_default();
    let file_size = std::fs::metadata(&file_path).map(|m| m.len()).unwrap_or(0);
    let filename = Path::new(&file_path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| format!("{}.bin", alloy::hex::encode(&content_hash_bytes[..8])));

    // 4. Build v2 metadata URI
    let metadata_uri = build_metadata_v2(
        &title,
        &description,
        &content_type,
        &filename,
        file_size,
        &node_id_str,
        &relay_url_str,
        &cats,
        main_img_asset.as_ref(),
        main_trailer_asset.as_ref(),
        &additional_assets,
    )?;

    // 5. Store metadata in SQLite (active = 0 until confirmed)
    let created_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let categories_json = serde_json::to_string(&cats).unwrap_or_default();

    {
        let db = state.db.lock().await;
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
                  filename, publisher_node_id, publisher_relay_url, categories)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, ?10, ?11, ?12, ?13, ?14)",
                rusqlite::params![
                    &content_hash_hex,
                    &content_hash_hex,
                    &creator,
                    &metadata_uri,
                    price_wei.to_string(),
                    &title,
                    &description,
                    &content_type,
                    file_size as i64,
                    created_at,
                    &filename,
                    &node_id_str,
                    &relay_url_str,
                    &categories_json,
                ],
            )
            .map_err(|e| format!("DB insert failed: {e}"))?;
    }

    info!("Metadata stored in DB for {}", content_hash_hex);

    // 6. Build on-chain publishContent calldata
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

    let content_id_hex = if tx_hash != "0x0" {
        match extract_content_id_from_receipt(&state, &tx_hash).await {
            Ok(id) => id,
            Err(receipt_err) => {
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
        content_hash.clone()
    };

    info!("On-chain contentId: {}", content_id_hex);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    {
        let db = state.db.lock().await;
        let conn = db.conn();

        match conn.execute(
            "UPDATE content SET content_id = ?1, active = 1, publisher_node_id = ?2, publisher_relay_url = ?3
             WHERE content_hash = ?4 AND active = 0",
            rusqlite::params![&content_id_hex, &node_id_str, &relay_url_str, &content_hash],
        ) {
            Ok(_) => {}
            Err(e) if e.to_string().contains("UNIQUE") => {
                conn.execute(
                    "DELETE FROM content WHERE content_hash = ?1 AND active = 0",
                    rusqlite::params![&content_hash],
                )
                .ok();
                info!("Cleaned up temp content row (sync had already created active row)");
            }
            Err(e) => return Err(format!("DB update failed: {e}")),
        }

        conn.execute(
            "INSERT OR IGNORE INTO seeding (content_id, active, bytes_served, peer_count, started_at)
             VALUES (?1, 1, 0, 0, ?2)",
            rusqlite::params![&content_id_hex, now],
        )
        .map_err(|e| format!("Seeding DB insert failed: {e}"))?;
        conn.execute(
            "UPDATE seeding SET active = 1 WHERE content_id = ?1",
            rusqlite::params![&content_id_hex],
        )
        .ok();
    }

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
#[tauri::command]
pub async fn update_content(
    state: State<'_, AppState>,
    content_id: String,
    title: String,
    description: String,
    content_type: String,
    price_eth: String,
    categories: Option<Vec<String>>,
) -> Result<Vec<TransactionRequest>, String> {
    info!(
        "Updating content: id={}, title={}, price={}",
        content_id, title, price_eth
    );

    let wallet = state.wallet_address.lock().await;
    let caller = wallet.as_ref().ok_or("No wallet connected")?.clone();
    drop(wallet);

    // Fetch preserved fields from DB
    let (old_filename, old_file_size, old_node_id, old_relay_url, old_metadata_uri) = {
        let db = state.db.lock().await;
        let conn = db.conn();
        conn.query_row(
            "SELECT creator, COALESCE(filename,''), COALESCE(file_size_bytes,0),
                    COALESCE(publisher_node_id,''), COALESCE(publisher_relay_url,''),
                    COALESCE(metadata_uri,'')
             FROM content WHERE content_id = ?1",
            rusqlite::params![&content_id],
            |row| {
                let creator: String = row.get(0)?;
                Ok((
                    creator,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .map(|(creator, filename, file_size, node_id, relay_url, meta)| {
            if creator.to_lowercase() != caller.to_lowercase() {
                Err(format!("Only the creator ({}) can edit this content", creator))
            } else {
                Ok((filename, file_size, node_id, relay_url, meta))
            }
        })
        .map_err(|e| format!("Content not found: {e}"))?
    }?;

    let price_wei = parse_token_amount(&price_eth)?;
    let cats = categories.unwrap_or_default();

    // Rebuild metadata preserving existing preview assets
    let metadata_uri = rebuild_metadata_preserving_previews(
        &old_metadata_uri,
        &title,
        &description,
        &content_type,
        &old_filename,
        old_file_size,
        &old_node_id,
        &old_relay_url,
        &cats,
    )?;

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
#[tauri::command]
pub async fn confirm_update_content(
    state: State<'_, AppState>,
    content_id: String,
    title: String,
    description: String,
    content_type: String,
    price_eth: String,
    categories: Option<Vec<String>>,
) -> Result<(), String> {
    info!("Confirming content update: id={}", content_id);

    let price_wei = parse_token_amount(&price_eth)?;
    let cats = categories.unwrap_or_default();
    let categories_json = serde_json::to_string(&cats).unwrap_or_default();

    let db = state.db.lock().await;
    let conn = db.conn();
    let (filename, file_size, node_id, relay_url, old_metadata_uri): (String, i64, String, String, String) = conn
        .query_row(
            "SELECT COALESCE(filename,''), COALESCE(file_size_bytes,0),
                    COALESCE(publisher_node_id,''), COALESCE(publisher_relay_url,''),
                    COALESCE(metadata_uri,'')
             FROM content WHERE content_id = ?1",
            rusqlite::params![&content_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )
        .map_err(|e| format!("Content not found: {e}"))?;

    let metadata_uri = rebuild_metadata_preserving_previews(
        &old_metadata_uri,
        &title,
        &description,
        &content_type,
        &filename,
        file_size,
        &node_id,
        &relay_url,
        &cats,
    )?;

    conn.execute(
        "UPDATE content SET title = ?1, description = ?2, content_type = ?3,
         price_wei = ?4, metadata_uri = ?5, categories = ?6
         WHERE content_id = ?7",
        rusqlite::params![
            &title, &description, &content_type,
            price_wei.to_string(), &metadata_uri, &categories_json, &content_id
        ],
    )
    .map_err(|e| format!("DB update failed: {e}"))?;

    info!("Content {} updated successfully", content_id);
    Ok(())
}

/// Prepare an on-chain updateContentFile transaction to replace the blob for an existing listing.
/// The contentId and purchase records remain unchanged — only the BLAKE3 hash changes.
#[tauri::command]
pub async fn update_content_file(
    state: State<'_, AppState>,
    content_id: String,
    file_path: String,
) -> Result<UpdateFileResult, String> {
    info!("Preparing file update: id={}, file={}", content_id, file_path);

    let wallet = state.wallet_address.lock().await;
    let caller = wallet.as_ref().ok_or("No wallet connected")?.clone();
    drop(wallet);

    // Verify creator
    {
        let db = state.db.lock().await;
        let creator: String = db
            .conn()
            .query_row(
                "SELECT creator FROM content WHERE content_id = ?1",
                rusqlite::params![&content_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("Content not found: {e}"))?;
        if creator.to_lowercase() != caller.to_lowercase() {
            return Err(format!("Only the creator ({}) can update this content file", creator));
        }
    }

    let file = Path::new(&file_path);
    if !file.exists() {
        return Err(format!("File not found: {}", file_path));
    }

    // Import new file to iroh
    let blobs_client = {
        let guard = state.ensure_iroh().await?;
        let node = guard.as_ref().unwrap();
        node.blobs_client()
    };
    let content_mgr = ContentManager::new(blobs_client);
    let new_hash_bytes = content_mgr
        .import_file(file)
        .await
        .map_err(|e| format!("Failed to import file: {e}"))?;
    let new_hash_hex = format!("0x{}", alloy::hex::encode(new_hash_bytes));
    info!("New file imported, BLAKE3 hash: {}", new_hash_hex);

    // Build on-chain updateContentFile calldata
    let registry_addr_str = &state.config.ethereum.registry_address;
    let registry_addr: Address = registry_addr_str
        .parse()
        .map_err(|e| format!("Invalid registry address: {e}"))?;

    let content_id_bytes = parse_content_hash_bytes(&content_id)?;
    let content_id_fixed = FixedBytes::from(content_id_bytes);
    let new_hash_fixed = FixedBytes::from(new_hash_bytes);

    let calldata =
        RegistryClient::<()>::update_content_file_calldata(content_id_fixed, new_hash_fixed);

    Ok(UpdateFileResult {
        new_content_hash: new_hash_hex,
        transactions: vec![TransactionRequest {
            to: format!("{registry_addr:#x}"),
            data: hex_encode(&calldata),
            value: "0x0".to_string(),
            description: "Update content file on Ara Marketplace".to_string(),
        }],
    })
}

/// Called after the updateContentFile transaction is confirmed.
/// Updates the DB, switches gossip topics from old hash to new hash.
#[tauri::command]
pub async fn confirm_content_file_update(
    state: State<'_, AppState>,
    content_id: String,
    new_content_hash: String,
) -> Result<(), String> {
    info!(
        "Confirming file update: id={}, new_hash={}",
        content_id, new_content_hash
    );

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    // Fetch old content_hash before updating
    let old_hash_hex: String = {
        let db = state.db.lock().await;
        db.conn()
            .query_row(
                "SELECT content_hash FROM content WHERE content_id = ?1",
                rusqlite::params![&content_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("Content not found: {e}"))?
    };

    // Also get node_id and relay_url for the updated metadata_uri
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

    {
        let db = state.db.lock().await;
        let conn = db.conn();

        // Update content_hash, updated_at, and publisher connection info
        conn.execute(
            "UPDATE content SET content_hash = ?1, updated_at = ?2,
             publisher_node_id = ?3, publisher_relay_url = ?4
             WHERE content_id = ?5",
            rusqlite::params![
                &new_content_hash, now, &node_id_str, &relay_url_str, &content_id
            ],
        )
        .map_err(|e| format!("DB update failed: {e}"))?;

        // Remove stale seeder records for old hash
        conn.execute(
            "DELETE FROM content_seeders WHERE content_hash = ?1",
            rusqlite::params![&old_hash_hex],
        )
        .ok();
    }

    // Leave old gossip topic, join new one
    if let Ok(old_hash) = parse_content_hash_bytes(&old_hash_hex) {
        let _ = state.send_gossip(GossipCmd::LeaveSeeding { content_hash: old_hash }).await;
    }

    let new_hash = parse_content_hash_bytes(&new_content_hash)?;
    state
        .send_gossip(GossipCmd::AnnounceSeeding {
            content_hash: new_hash,
            bootstrap: vec![],
        })
        .await?;

    info!(
        "File update confirmed for {} — old={}, new={}",
        content_id, old_hash_hex, new_content_hash
    );
    Ok(())
}

/// Import preview asset files into iroh and return their hashes + metadata.
/// Call this before `publish_content` to get preview asset info to embed in metadata.
#[tauri::command]
pub async fn import_preview_assets(
    state: State<'_, AppState>,
    file_paths: Vec<String>,
) -> Result<Vec<PreviewAsset>, String> {
    info!("Importing {} preview assets", file_paths.len());

    let blobs_client = {
        let guard = state.ensure_iroh().await?;
        let node = guard.as_ref().unwrap();
        node.blobs_client()
    };
    let content_mgr = ContentManager::new(blobs_client);
    import_preview_files(&content_mgr, &file_paths).await
}

/// Download a preview blob from the content publisher and return its local file path.
/// The frontend can then use `convertFileSrc(path)` to display the preview.
#[tauri::command]
pub async fn get_preview_asset(
    state: State<'_, AppState>,
    content_id: String,
    preview_hash: String,
    filename: String,
) -> Result<String, String> {
    info!(
        "Fetching preview asset: content={}, hash={}, filename={}",
        content_id, preview_hash, filename
    );

    let (publisher_node_id_opt, publisher_relay_url_opt) = {
        let db = state.db.lock().await;
        let conn = db.conn();
        conn.query_row(
            "SELECT publisher_node_id, publisher_relay_url FROM content WHERE content_id = ?1",
            rusqlite::params![&content_id],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                ))
            },
        )
        .map(|(n, r)| {
            (
                n.filter(|s| !s.is_empty()),
                r.filter(|s| !s.is_empty()),
            )
        })
        .map_err(|e| format!("Content not found: {e}"))?
    };

    let preview_hash_bytes = parse_content_hash_bytes(&preview_hash)?;

    let (blobs_client, our_node_id_str) = {
        let guard = state.ensure_iroh().await?;
        let node = guard.as_ref().unwrap();
        (node.blobs_client(), node.node_id().to_string())
    };
    let content_mgr = ContentManager::new(blobs_client);

    // Download from publisher if not already local
    let already_local = content_mgr
        .has_blob(&preview_hash_bytes)
        .await
        .unwrap_or(false);

    let is_self = publisher_node_id_opt.as_deref() == Some(our_node_id_str.as_str());

    if !already_local && !is_self {
        if let Some(ref node_id_str) = publisher_node_id_opt {
            let node_id: iroh::NodeId = node_id_str
                .parse()
                .map_err(|e| format!("Invalid publisher node ID: {e}"))?;
            let mut node_addr = iroh::NodeAddr::from(node_id);
            if let Some(relay_url) = publisher_relay_url_opt
                .as_deref()
                .filter(|u| !u.is_empty())
            {
                if let Ok(url) = relay_url.parse() {
                    node_addr = node_addr.with_relay_url(url);
                }
            }
            content_mgr
                .download_from(&preview_hash_bytes, node_addr)
                .await
                .map_err(|e| format!("Preview download failed: {e}"))?;
        } else {
            return Err("Publisher node ID not available — cannot fetch preview".to_string());
        }
    }

    // Export to preview_cache directory
    let preview_cache_dir = Path::new(&state.config.storage.downloads_dir)
        .parent()
        .unwrap_or(Path::new("."))
        .join("preview_cache");
    std::fs::create_dir_all(&preview_cache_dir)
        .map_err(|e| format!("Failed to create preview_cache dir: {e}"))?;

    let hash_prefix = alloy::hex::encode(&preview_hash_bytes[..8]);
    let safe_filename = Path::new(&filename)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| format!("{}.bin", hash_prefix));
    let output_path = preview_cache_dir.join(format!("{}_{}", hash_prefix, safe_filename));

    if !output_path.exists() {
        content_mgr
            .export_blob(&preview_hash_bytes, &output_path)
            .await
            .map_err(|e| format!("Preview export failed: {e}"))?;
    }

    Ok(output_path.to_string_lossy().into_owned())
}

// ─── Read queries ─────────────────────────────────────────────────────────────

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
                    content_type, price_wei, active, file_size_bytes,
                    COALESCE(metadata_uri,''), updated_at, COALESCE(categories,'[]')
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
            let cats_json: String = row.get(11)?;
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
                metadata_uri: row.get(9)?,
                updated_at: row.get(10)?,
                categories: serde_json::from_str(&cats_json).unwrap_or_default(),
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

    {
        let db = state.db.lock().await;
        let creator: String = db
            .conn()
            .query_row(
                "SELECT creator FROM content WHERE content_id = ?1",
                rusqlite::params![&content_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("Content not found: {e}"))?;
        if creator.to_lowercase() != caller.to_lowercase() {
            return Err(format!(
                "Only the creator ({}) can delist this content",
                creator
            ));
        }
    }

    let registry_addr_str = &state.config.ethereum.registry_address;
    let registry_addr: Address = if registry_addr_str.is_empty() {
        Address::ZERO
    } else {
        registry_addr_str
            .parse()
            .map_err(|e| format!("Invalid registry address: {e}"))?
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

/// Called after the delist transaction is confirmed.
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
            .execute(
                "UPDATE content SET active = 0 WHERE content_id = ?1",
                rusqlite::params![&content_id],
            )
            .map_err(|e| format!("DB update failed: {e}"))?;
        db.conn()
            .execute(
                "UPDATE seeding SET active = 0 WHERE content_id = ?1",
                rusqlite::params![&content_id],
            )
            .ok();
    }

    if let Ok(content_hash) = parse_content_hash_bytes(&content_hash_hex) {
        let _ = state
            .send_gossip(GossipCmd::LeaveSeeding { content_hash })
            .await;
    }

    info!("Content {} delisted successfully", content_id);
    Ok(())
}

/// Return all active content published by the connected wallet, with seeding status.
#[tauri::command]
pub async fn get_published_content(
    state: State<'_, AppState>,
) -> Result<Vec<PublishedItem>, String> {
    let wallet = state.wallet_address.lock().await;
    let creator = wallet.as_ref().ok_or("No wallet connected")?.to_lowercase();
    drop(wallet);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let db = state.db.lock().await;
    let conn = db.conn();

    let mut stmt = conn
        .prepare(
            "SELECT c.content_id, c.title, c.content_type, c.price_wei,
                    COALESCE(s.active, 0) as is_seeding,
                    COALESCE(c.file_size_bytes, 0),
                    c.updated_at
             FROM content c
             LEFT JOIN seeding s ON c.content_id = s.content_id
             WHERE LOWER(c.creator) = ?1 AND c.active = 1
             ORDER BY c.created_at DESC",
        )
        .map_err(|e| format!("DB query failed: {e}"))?;

    let rows = stmt
        .query_map(rusqlite::params![&creator], |row| {
            let price_wei_str: String = row.get(3)?;
            let price_wei = price_wei_str
                .parse::<alloy::primitives::U256>()
                .unwrap_or(alloy::primitives::U256::ZERO);
            Ok(PublishedItem {
                content_id: row.get(0)?,
                title: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                content_type: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                price_eth: format_wei(price_wei),
                is_seeding: row.get::<_, i32>(4)? != 0,
                file_size_bytes: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })
        .map_err(|e| format!("DB query failed: {e}"))?;

    let mut items: Vec<PublishedItem> = Vec::new();
    for row in rows {
        items.push(row.map_err(|e| format!("Row parse error: {e}"))?);
    }

    // Auto-create seeding entries for published items missing one
    for item in &mut items {
        if !item.is_seeding {
            let inserted = conn.execute(
                "INSERT OR IGNORE INTO seeding (content_id, active, bytes_served, peer_count, started_at)
                 VALUES (?1, 1, 0, 0, ?2)",
                rusqlite::params![&item.content_id, now],
            );
            if inserted.map(|n| n > 0).unwrap_or(false) {
                info!(
                    "Auto-created seeding entry for published content {}",
                    item.content_id
                );
                item.is_seeding = true;
            }
        }
    }

    info!("Published content: {} items", items.len());
    Ok(items)
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
                    content_type, price_wei, active, file_size_bytes,
                    COALESCE(metadata_uri,''), updated_at, COALESCE(categories,'[]')
             FROM content WHERE content_id = ?1",
        )
        .map_err(|e| format!("DB query failed: {e}"))?;

    let detail = stmt
        .query_row(rusqlite::params![&content_id], |row| {
            let price_wei_str: String = row.get(6)?;
            let price_wei = price_wei_str
                .parse::<alloy::primitives::U256>()
                .unwrap_or(alloy::primitives::U256::ZERO);
            let cats_json: String = row.get(11)?;
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
                metadata_uri: row.get(9)?,
                updated_at: row.get(10)?,
                categories: serde_json::from_str(&cats_json).unwrap_or_default(),
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
                    content_type, price_wei, active, file_size_bytes,
                    COALESCE(metadata_uri,''), updated_at, COALESCE(categories,'[]')
             FROM content WHERE active = 1
             AND (title LIKE ?1 OR description LIKE ?1
                  OR content_type LIKE ?1 OR COALESCE(categories,'') LIKE ?1)
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
            let cats_json: String = row.get(11)?;
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
                metadata_uri: row.get(9)?,
                updated_at: row.get(10)?,
                categories: serde_json::from_str(&cats_json).unwrap_or_default(),
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

// ─── Utility ──────────────────────────────────────────────────────────────────

/// Parse a 0x-prefixed hex string into a 32-byte array.
fn parse_content_hash_bytes(s: &str) -> Result<[u8; 32], String> {
    let hex_str = s.strip_prefix("0x").unwrap_or(s);
    let bytes =
        alloy::hex::decode(hex_str).map_err(|e| format!("Invalid content hash: {e}"))?;
    if bytes.len() != 32 {
        return Err(format!(
            "Content hash must be 32 bytes, got {}",
            bytes.len()
        ));
    }
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&bytes);
    Ok(hash)
}
