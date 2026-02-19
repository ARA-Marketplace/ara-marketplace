use crate::commands::types::{format_wei, hex_encode, TransactionRequest};
use crate::state::AppState;
use alloy::primitives::{Address, FixedBytes, U256};
use ara_chain::marketplace::MarketplaceClient;
use ara_p2p::content::ContentManager;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tauri::State;
use tracing::info;

#[derive(Serialize, Deserialize)]
pub struct PurchasePrepareResult {
    /// Content ID (0x-prefixed hex)
    pub content_id: String,
    /// Title of the content
    pub title: String,
    /// Price in ETH (decimal string)
    pub price_eth: String,
    /// Transactions to sign (purchase call with ETH value)
    pub transactions: Vec<TransactionRequest>,
}

#[derive(Serialize, Deserialize)]
pub struct LibraryItem {
    pub content_id: String,
    pub title: String,
    pub content_type: String,
    pub purchased_at: u64,
    pub is_seeding: bool,
    pub download_path: Option<String>,
    pub tx_hash: Option<String>,
}

/// Step 1: Build a purchase transaction for frontend signing.
/// Looks up the content in local DB, builds marketplace.purchase(contentId) calldata
/// with the correct ETH value attached.
#[tauri::command]
pub async fn purchase_content(
    state: State<'_, AppState>,
    content_id: String,
) -> Result<PurchasePrepareResult, String> {
    info!("Preparing purchase for content: {}", content_id);

    // Require wallet
    let wallet = state.wallet_address.lock().await;
    let buyer_addr_str = wallet.as_ref().ok_or("No wallet connected")?.clone();
    drop(wallet);

    // Look up content in local DB to get price and title
    let (title, price_wei_str, _content_type) = {
        let db = state.db.lock().await;
        let conn = db.conn();
        conn.query_row(
            "SELECT title, price_wei, content_type FROM content
             WHERE content_id = ?1 AND active = 1",
            rusqlite::params![&content_id],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                ))
            },
        )
        .map_err(|e| format!("Content not found or not active: {e}"))?
    };

    let price_wei: U256 = price_wei_str
        .parse()
        .map_err(|e| format!("Invalid price in DB: {e}"))?;
    let price_eth = format_wei(price_wei);

    // Build purchase transaction
    let marketplace_addr_str = &state.config.ethereum.marketplace_address;
    let marketplace_addr = if marketplace_addr_str.is_empty() {
        Address::ZERO
    } else {
        marketplace_addr_str
            .parse::<Address>()
            .map_err(|e| format!("Invalid marketplace address: {e}"))?
    };

    let transactions = if marketplace_addr == Address::ZERO {
        info!("Marketplace not deployed — local-only purchase");
        vec![]
    } else {
        let content_id_bytes: FixedBytes<32> = content_id
            .strip_prefix("0x")
            .unwrap_or(&content_id)
            .parse()
            .map_err(|e| format!("Invalid content ID: {e}"))?;

        // Check on-chain if already purchased (handles the case where the purchase tx
        // confirmed but confirm_purchase never ran locally, e.g. due to a hang).
        let already_purchased_onchain = if let Ok(chain) = state.chain_client() {
            let buyer_addr: Address = buyer_addr_str
                .parse()
                .unwrap_or(Address::ZERO);
            chain
                .marketplace
                .has_purchased(content_id_bytes, buyer_addr)
                .await
                .unwrap_or(false)
        } else {
            false
        };

        if already_purchased_onchain {
            info!(
                "Content {} already purchased on-chain — skipping tx, running confirm directly",
                content_id
            );
            vec![] // empty = local-only path, confirm_purchase will record it
        } else {
            let calldata = MarketplaceClient::<()>::purchase_calldata(content_id_bytes);

            // value = price in wei (hex-encoded) — this is a payable call
            let value_hex = format!("0x{:x}", price_wei);

            vec![TransactionRequest {
                to: format!("{marketplace_addr:#x}"),
                data: hex_encode(&calldata),
                value: value_hex,
                description: format!("Purchase \"{}\" for {} ETH", title, price_eth),
            }]
        }
    };

    Ok(PurchasePrepareResult {
        content_id: content_id.clone(),
        title,
        price_eth,
        transactions,
    })
}

/// Step 2: Called after the purchase transaction is confirmed (or immediately for local-only).
/// Records the purchase in the local DB.
#[tauri::command]
pub async fn confirm_purchase(
    state: State<'_, AppState>,
    content_id: String,
    tx_hash: String,
) -> Result<(), String> {
    info!(
        "Confirming purchase: content={}, tx={}",
        content_id, tx_hash
    );

    let wallet = state.wallet_address.lock().await;
    let buyer = wallet.as_ref().ok_or("No wallet connected")?.clone();
    drop(wallet);

    // Get price from content table
    let price_wei_str = {
        let db = state.db.lock().await;
        db.conn()
            .query_row(
                "SELECT price_wei FROM content WHERE content_id = ?1",
                rusqlite::params![&content_id],
                |row| row.get::<_, String>(0),
            )
            .map_err(|e| format!("Content not found: {e}"))?
    };

    let purchased_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    // Get content_hash (BLAKE3, for iroh), publisher node ID, and filename from content table
    let (content_hash_str, publisher_node_id_opt, filename_opt) = {
        let db = state.db.lock().await;
        db.conn()
            .query_row(
                "SELECT content_hash, publisher_node_id, filename FROM content WHERE content_id = ?1",
                rusqlite::params![&content_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .map_err(|e| format!("Content not found: {e}"))?
    };

    // Insert into purchases table
    {
        let db = state.db.lock().await;
        db.conn()
            .execute(
                "INSERT OR REPLACE INTO purchases
                 (content_id, buyer, price_paid_wei, tx_hash, purchased_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![&content_id, &buyer, &price_wei_str, &tx_hash, purchased_at],
            )
            .map_err(|e| format!("DB insert failed: {e}"))?;
    }

    info!("Purchase recorded for {} by {}", content_id, buyer);

    // --- P2P Download ---
    // content_hash_str is the BLAKE3 hash (iroh blob identifier).
    // content_id is the keccak256-derived on-chain ID — do NOT use it for iroh.
    let content_hash_bytes = parse_content_hash_bytes(&content_hash_str)?;

    // Ensure downloads directory exists
    let downloads_dir = Path::new(&state.config.storage.downloads_dir);
    std::fs::create_dir_all(downloads_dir)
        .map_err(|e| format!("Failed to create downloads dir: {e}"))?;

    // Use stored filename if available; otherwise use a temp name and detect type after export
    let known_filename = filename_opt.filter(|f| {
        // Only trust stored filename if it has a meaningful extension
        Path::new(f).extension().map_or(false, |e| e != "bin")
    });
    let hash_prefix = alloy::hex::encode(&content_hash_bytes[..8]);
    let temp_filename = format!("{}.bin", hash_prefix);
    let output_path = downloads_dir.join(known_filename.as_deref().unwrap_or(&temp_filename));

    // Get blobs client and current node ID (drop iroh guard before any async work)
    let (blobs_client, our_node_id_str) = {
        let guard = state.ensure_iroh().await?;
        let node = guard.as_ref().unwrap();
        (node.blobs_client(), node.node_id().to_string())
    };
    let content_mgr = ContentManager::new(blobs_client);

    // Download from publisher if blob not already in local store.
    // Skip download if we ARE the publisher (blob is already in our iroh store).
    let already_local = content_mgr
        .has_blob(&content_hash_bytes)
        .await
        .unwrap_or(false);

    let is_self = publisher_node_id_opt.as_deref() == Some(our_node_id_str.as_str());

    if !already_local && !is_self {
        if let Some(node_id_str) = &publisher_node_id_opt {
            let node_id: iroh::NodeId = node_id_str
                .parse()
                .map_err(|e| format!("Invalid publisher node ID: {e}"))?;
            let node_addr = iroh::NodeAddr::from(node_id);
            info!("Downloading blob from publisher {}", node_id_str);
            content_mgr
                .download_from(&content_hash_bytes, node_addr)
                .await
                .map_err(|e| format!("P2P download failed: {e}"))?;
        } else {
            return Err("Publisher node ID not available — cannot download content".to_string());
        }
    } else if already_local {
        info!("Blob already in local store, skipping download");
    } else {
        info!("Buyer is the publisher — blob already in local store");
    }

    // Export blob to downloads directory
    content_mgr
        .export_blob(&content_hash_bytes, &output_path)
        .await
        .map_err(|e| format!("Export to file failed: {e}"))?;

    // If filename had no real extension, detect file type from magic bytes and rename
    let output_path = if known_filename.is_none() {
        detect_and_rename(output_path, &hash_prefix)
    } else {
        output_path
    };

    let download_path_str = output_path.to_string_lossy().into_owned();
    info!("Content saved to {}", download_path_str);

    // Store the download path in purchases table
    {
        let db = state.db.lock().await;
        db.conn()
            .execute(
                "UPDATE purchases SET downloaded_path = ?1
                 WHERE content_id = ?2 AND buyer = ?3",
                rusqlite::params![&download_path_str, &content_id, &buyer],
            )
            .map_err(|e| format!("DB update failed: {e}"))?;
    }

    Ok(())
}

/// Get the user's library of purchased content.
#[tauri::command]
pub async fn get_library(state: State<'_, AppState>) -> Result<Vec<LibraryItem>, String> {
    info!("Fetching user library");

    let wallet = state.wallet_address.lock().await;
    let buyer = wallet.as_ref().ok_or("No wallet connected")?.clone();
    drop(wallet);

    let db = state.db.lock().await;
    let conn = db.conn();

    let mut stmt = conn
        .prepare(
            "SELECT p.content_id, c.title, c.content_type, p.purchased_at,
                    COALESCE(s.active, 0) as is_seeding, p.downloaded_path, p.tx_hash
             FROM purchases p
             LEFT JOIN content c ON p.content_id = c.content_id
             LEFT JOIN seeding s ON p.content_id = s.content_id
             WHERE p.buyer = ?1
             ORDER BY p.purchased_at DESC",
        )
        .map_err(|e| format!("DB query failed: {e}"))?;

    let rows = stmt
        .query_map(rusqlite::params![&buyer], |row| {
            Ok(LibraryItem {
                content_id: row.get(0)?,
                title: row.get::<_, Option<String>>(1)?.unwrap_or("Unknown".to_string()),
                content_type: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                purchased_at: row.get::<_, i64>(3)? as u64,
                is_seeding: row.get::<_, i32>(4)? != 0,
                download_path: row.get::<_, Option<String>>(5)?,
                tx_hash: row.get::<_, Option<String>>(6)?,
            })
        })
        .map_err(|e| format!("DB query failed: {e}"))?;

    let mut items = Vec::new();
    for row in rows {
        items.push(row.map_err(|e| format!("Row parse error: {e}"))?);
    }

    info!("Library has {} items for {}", items.len(), buyer);
    Ok(items)
}

/// Open a downloaded content file with the OS default application.
/// Returns the file path that was opened.
#[tauri::command]
pub async fn open_downloaded_content(
    state: State<'_, AppState>,
    content_id: String,
) -> Result<String, String> {
    let wallet = state.wallet_address.lock().await;
    let buyer = wallet.as_ref().ok_or("No wallet connected")?.clone();
    drop(wallet);

    let db = state.db.lock().await;
    let downloaded_path: Option<String> = db
        .conn()
        .query_row(
            "SELECT downloaded_path FROM purchases WHERE content_id = ?1 AND buyer = ?2",
            rusqlite::params![&content_id, &buyer],
            |row| row.get(0),
        )
        .map_err(|e| format!("Purchase not found: {e}"))?;
    drop(db);

    let path = downloaded_path.ok_or("Content not yet downloaded")?;
    opener::open(&path).map_err(|e| format!("Failed to open file: {e}"))?;
    Ok(path)
}

/// Read the first 16 bytes of a file to detect its MIME type, then rename with
/// the correct extension. Falls back to the original path if detection fails.
fn detect_and_rename(path: std::path::PathBuf, hash_prefix: &str) -> std::path::PathBuf {
    use std::io::Read;
    let detected = std::fs::File::open(&path)
        .ok()
        .and_then(|mut f| {
            let mut buf = [0u8; 16];
            let n = f.read(&mut buf).unwrap_or(0);
            infer::get(&buf[..n]).map(|k| k.extension().to_string())
        });

    if let Some(ext) = detected {
        let new_path = path.parent().unwrap_or(std::path::Path::new("."))
            .join(format!("{}.{}", hash_prefix, ext));
        if std::fs::rename(&path, &new_path).is_ok() {
            info!("Detected file type: .{} → renamed to {}", ext, new_path.display());
            return new_path;
        }
    }
    path
}

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
