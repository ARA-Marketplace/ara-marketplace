use crate::commands::types::{format_token_amount, format_wei, hex_encode, parse_token_amount, TransactionRequest};
use ara_chain::token::TokenClient;
use crate::gossip_actor::GossipCmd;
use crate::state::AppState;
use alloy::primitives::{Address, FixedBytes, U256};
use ara_chain::content_token::ContentTokenClient;
use ara_chain::marketplace::MarketplaceClient;
use ara_p2p::content::ContentManager;
use iroh_blobs::net_protocol::DownloadMode;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;
use tauri::{Emitter, State};
use tracing::{info, warn};

/// How long to wait for the P2P download before giving up. If no seeder responds within
/// this window, we return an error so the UI can show the "File missing / Redownload"
/// CTA instead of hanging the purchase indefinitely. The buyer's purchase is already
/// recorded on-chain and in the `purchases` DB table at this point.
const P2P_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Serialize, Deserialize)]
pub struct ConfirmPurchaseResult {
    /// Local filesystem path where the content was saved
    pub download_path: String,
}

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

    // Look up content in local DB to get price, title, and payment token
    let (title, price_wei_str, _content_type, payment_token_str) = {
        let db = state.db.lock().await;
        let conn = db.conn();
        conn.query_row(
            "SELECT title, price_wei, content_type, COALESCE(payment_token, '') FROM content
             WHERE content_id = ?1 AND active = 1",
            rusqlite::params![&content_id],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .map_err(|e| format!("Content not found or not active: {e}"))?
    };

    let price_wei: U256 = price_wei_str
        .parse()
        .map_err(|e| format!("Invalid price in DB: {e}"))?;

    // Determine if this is a token purchase
    let is_token_purchase = !payment_token_str.is_empty()
        && payment_token_str != "0x0000000000000000000000000000000000000000";

    // Format price with correct decimals
    let (price_display, price_unit) = if is_token_purchase {
        let token_cfg = state.config.ethereum.supported_tokens.iter()
            .find(|t| t.address.eq_ignore_ascii_case(&payment_token_str));
        let decimals = token_cfg.map(|t| t.decimals).unwrap_or(18);
        let symbol = token_cfg.map(|t| t.symbol.as_str()).unwrap_or("TOKEN");
        (format_token_amount(price_wei, decimals), symbol.to_string())
    } else {
        (format_wei(price_wei), "ETH".to_string())
    };
    let price_eth = price_display.clone();

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
        } else if is_token_purchase {
            // Token purchase: approve token + purchaseWithToken
            let token_addr: Address = payment_token_str
                .parse()
                .map_err(|e| format!("Invalid payment token address: {e}"))?;

            let approve_calldata = TokenClient::<()>::approve_calldata(marketplace_addr, price_wei);
            let purchase_calldata = MarketplaceClient::<()>::purchase_with_token_calldata(
                content_id_bytes,
                token_addr,
                price_wei,
                price_wei, // maxPrice = price (slippage protection)
            );

            vec![
                TransactionRequest {
                    to: format!("{token_addr:#x}"),
                    data: hex_encode(&approve_calldata),
                    value: "0x0".to_string(),
                    description: format!("Approve {} {} for marketplace", price_display, price_unit),
                },
                TransactionRequest {
                    to: format!("{marketplace_addr:#x}"),
                    data: hex_encode(&purchase_calldata),
                    value: "0x0".to_string(),
                    description: format!("Purchase \"{}\" for {} {}", title, price_display, price_unit),
                },
            ]
        } else {
            let calldata = MarketplaceClient::<()>::purchase_calldata(content_id_bytes, price_wei);

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
) -> Result<ConfirmPurchaseResult, String> {
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

    // Get content_hash (BLAKE3, for iroh), publisher node ID, relay URL, filename, and arweave_tx_id from content table
    let (content_hash_str, publisher_node_id_opt, publisher_relay_url_opt, filename_opt, arweave_tx_id_opt) = {
        let db = state.db.lock().await;
        let row = db.conn()
            .query_row(
                "SELECT content_hash, publisher_node_id, publisher_relay_url, filename, arweave_tx_id FROM content WHERE content_id = ?1",
                rusqlite::params![&content_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                    ))
                },
            )
            .map_err(|e| format!("Content not found: {e}"))?;
        // Filter empty strings — legacy content may store "" instead of NULL
        (row.0, row.1.filter(|s| !s.is_empty()), row.2.filter(|s| !s.is_empty()), row.3, row.4.filter(|s| !s.is_empty()))
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

    // Use stored filename if available; otherwise use a temp name and detect type after export.
    // SECURITY: strip path components to prevent directory traversal (e.g. "../../evil.exe")
    let known_filename = filename_opt
        .as_deref()
        .and_then(|f| Path::new(f).file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|f| {
            // Only trust stored filename if it has a meaningful extension
            Path::new(f).extension().map_or(false, |e| e != "bin")
        });
    let hash_prefix = alloy::hex::encode(&content_hash_bytes[..8]);
    let temp_filename = format!("{}.bin", hash_prefix);
    let output_path = downloads_dir.join(known_filename.as_deref().unwrap_or(&temp_filename));

    // Get blobs client, node ID, and endpoint (drop iroh guard before any async work)
    let (blobs_client, our_node_id_str, endpoint) = {
        let guard = state.ensure_iroh().await?;
        let node = guard.as_ref().unwrap();
        (node.blobs_client(), node.node_id().to_string(), node.endpoint().clone())
    };
    let content_mgr = ContentManager::new(blobs_client);

    // Download from publisher if blob not already in local store.
    // Skip download if we ARE the publisher (blob is already in our iroh store).
    let already_local = content_mgr
        .has_blob(&content_hash_bytes)
        .await
        .unwrap_or(false);

    let is_self = publisher_node_id_opt.as_deref() == Some(our_node_id_str.as_str());

    // Track whether we downloaded from Arweave (skip iroh export if so)
    let mut downloaded_from_arweave = false;

    if !already_local && !is_self {
        // Try P2P download first
        let p2p_result = if let Some(node_id_str) = &publisher_node_id_opt {
            let node_id: iroh::NodeId = node_id_str
                .parse()
                .map_err(|e| format!("Invalid publisher node ID: {e}"))?;

            // Build NodeAddr with relay URL so iroh can actually connect to the publisher
            let mut node_addr = iroh::NodeAddr::from(node_id);
            if let Some(relay_url) = publisher_relay_url_opt
                .as_deref()
                .filter(|u| !u.is_empty())
            {
                if let Ok(url) = relay_url.parse() {
                    node_addr = node_addr.with_relay_url(url);
                    info!(
                        "Downloading blob from publisher {} via relay {}",
                        node_id_str, relay_url
                    );
                } else {
                    info!(
                        "Downloading blob from publisher {} (relay URL parse failed: {})",
                        node_id_str, relay_url
                    );
                }
            } else {
                info!(
                    "Downloading blob from publisher {} (no relay URL, relying on discovery)",
                    node_id_str
                );
            }

            let app_handle = state.app_handle.clone();
            let progress_content_id = content_id.clone();
            let download_fut = content_mgr.download_with_progress(
                &content_hash_bytes,
                node_addr,
                DownloadMode::Queued,
                move |received, total| {
                    let _ = app_handle.emit(
                        "download-progress",
                        serde_json::json!({
                            "content_id": progress_content_id,
                            "bytes_received": received,
                            "total_bytes": total,
                        }),
                    );
                },
            );
            match tokio::time::timeout(P2P_DOWNLOAD_TIMEOUT, download_fut).await {
                Ok(r) => r,
                Err(_) => {
                    warn!(
                        "P2P download timed out after {}s for {} — publisher likely offline",
                        P2P_DOWNLOAD_TIMEOUT.as_secs(),
                        content_id
                    );
                    Err(anyhow::anyhow!(
                        "P2P download timed out — no seeders responded"
                    ))
                }
            }
        } else {
            Err(anyhow::anyhow!("Publisher node ID not available"))
        };

        if let Err(p2p_err) = p2p_result {
            // P2P download failed — try Arweave fallback if available
            if let Some(ref tx_id) = arweave_tx_id_opt {
                info!(
                    "P2P download failed ({}), falling back to Arweave tx {}",
                    p2p_err, tx_id
                );

                let arweave_config = crate::arweave::IrysConfig {
                    node_url: state.config.arweave.node_url.clone(),
                    gateway_url: state.config.arweave.gateway_url.clone(),
                };
                let client = crate::arweave::http_client_large_transfer();
                let bytes = crate::arweave::download_from_arweave(&client, &arweave_config, tx_id)
                    .await
                    .map_err(|e| format!("Arweave fallback download also failed: {e}"))?;

                // Write directly to output path
                std::fs::write(&output_path, &bytes)
                    .map_err(|e| format!("Failed to save Arweave download: {e}"))?;
                downloaded_from_arweave = true;

                info!(
                    "Downloaded {} bytes from Arweave permanent storage (no active seeders)",
                    bytes.len()
                );
            } else {
                return Err(format!("P2P download failed: {p2p_err}"));
            }
        }
    } else if already_local {
        info!("Blob already in local store, skipping download");
    } else {
        info!("Buyer is the publisher — blob already in local store");
    }

    // Export blob to downloads directory (skip if already downloaded from Arweave)
    // Also skip if the file already exists (e.g. re-confirming a previous purchase)
    if downloaded_from_arweave {
        info!("File saved directly from Arweave, skipping iroh export");
    } else if output_path.exists() {
        info!("File already exists at {}, skipping export", output_path.display());
    } else {
        content_mgr
            .export_blob(&content_hash_bytes, &output_path)
            .await
            .map_err(|e| format!("Export to file failed: {e}"))?;
    }

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

    // --- Auto-start seeding after download ---
    // The buyer now has the blob; announce on gossip so other buyers can download from us too.
    {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let db = state.db.lock().await;
        db.conn()
            .execute(
                "INSERT OR REPLACE INTO seeding (content_id, active, bytes_served, peer_count, started_at)
                 VALUES (?1, 1, 0, 0, ?2)",
                rusqlite::params![&content_id, now],
            )
            .map_err(|e| format!("Seeding DB insert failed: {e}"))?;
    }

    // Build bootstrap: use the publisher's NodeId (we just downloaded from them)
    let bootstrap: Vec<iroh::NodeId> = publisher_node_id_opt
        .as_deref()
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse::<iroh::NodeId>().ok())
        .into_iter()
        .collect();

    // Explicitly add the publisher's relay URL to the endpoint routing table so gossip
    // can dial them for bootstrap. iroh blob download_with_opts may not populate the
    // shared routing table, so we do it explicitly here.
    for &bootstrap_id in &bootstrap {
        let mut addr = iroh::NodeAddr::from(bootstrap_id);
        if let Some(relay_url_str) = publisher_relay_url_opt.as_deref().filter(|u| !u.is_empty()) {
            if let Ok(relay_url) = relay_url_str.parse() {
                addr = addr.with_relay_url(relay_url);
            }
        }
        let _ = endpoint.add_node_addr(addr);
    }

    state
        .send_gossip(GossipCmd::AnnounceSeeding {
            content_hash: content_hash_bytes,
            bootstrap,
        })
        .await?;

    info!("Auto-started seeding for {} after purchase", content_id);

    Ok(ConfirmPurchaseResult {
        download_path: download_path_str,
    })
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

/// Return the local path of a downloaded content file if the current wallet has purchased it.
/// Returns None when the viewer doesn't own the content or hasn't downloaded it yet.
/// Used by the frontend to render an inline media player for owned content.
#[tauri::command]
pub async fn get_owned_content_path(
    state: State<'_, AppState>,
    content_id: String,
) -> Result<Option<String>, String> {
    let wallet = state.wallet_address.lock().await;
    let Some(buyer) = wallet.as_ref().cloned() else {
        return Ok(None);
    };
    drop(wallet);

    let db = state.db.lock().await;
    let row = db.conn().query_row(
        "SELECT downloaded_path FROM purchases WHERE content_id = ?1 AND buyer = ?2",
        rusqlite::params![&content_id, &buyer],
        |row| row.get::<_, Option<String>>(0),
    );
    drop(db);

    let downloaded_path = match row {
        Ok(p) => p,
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
        Err(e) => return Err(format!("DB error: {e}")),
    };
    let Some(path) = downloaded_path else {
        return Ok(None);
    };

    // SECURITY: same check as open_downloaded_content — the path must be inside
    // the downloads directory to prevent rendering arbitrary local files. If the
    // file was moved or deleted, canonicalize fails — return None so the frontend
    // can offer a redownload button instead of erroring out.
    let canonical = match std::fs::canonicalize(&path) {
        Ok(p) => p,
        Err(_) => return Ok(None),
    };
    let downloads_base = std::fs::canonicalize(&state.config.storage.downloads_dir)
        .map_err(|e| format!("Downloads dir error: {e}"))?;
    if !canonical.starts_with(&downloads_base) {
        return Err("Security: file is not in downloads directory".into());
    }

    Ok(Some(path))
}

/// Return true if the current wallet purchased this content (based on the `purchases` DB).
/// Used by the frontend to distinguish "not owned" from "owned but file missing".
#[tauri::command]
pub async fn has_purchased_content(
    state: State<'_, AppState>,
    content_id: String,
) -> Result<bool, String> {
    let wallet = state.wallet_address.lock().await;
    let Some(buyer) = wallet.as_ref().cloned() else {
        return Ok(false);
    };
    drop(wallet);

    let db = state.db.lock().await;
    let count: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM purchases WHERE content_id = ?1 AND buyer = ?2",
            rusqlite::params![&content_id, &buyer],
            |row| row.get(0),
        )
        .unwrap_or(0);
    Ok(count > 0)
}

/// Re-download content the viewer previously purchased but has since moved or deleted.
/// Looks up the content hash + publisher from the DB and re-runs the P2P fetch + export.
/// Returns the new local path on success.
#[tauri::command]
pub async fn redownload_content(
    state: State<'_, AppState>,
    content_id: String,
) -> Result<String, String> {
    let wallet = state.wallet_address.lock().await;
    let buyer = wallet.as_ref().ok_or("No wallet connected")?.clone();
    drop(wallet);

    // Verify the wallet actually owns this content
    {
        let db = state.db.lock().await;
        let count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM purchases WHERE content_id = ?1 AND buyer = ?2",
                rusqlite::params![&content_id, &buyer],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if count == 0 {
            return Err("You haven't purchased this content.".into());
        }
    }

    // Look up content details from the content table — same fields used by confirm_purchase
    let (content_hash_str, publisher_node_id_opt, publisher_relay_url_opt, filename_opt) = {
        let db = state.db.lock().await;
        let row = db
            .conn()
            .query_row(
                "SELECT content_hash, publisher_node_id, publisher_relay_url, filename
                 FROM content WHERE content_id = ?1",
                rusqlite::params![&content_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .map_err(|e| format!("Content not found: {e}"))?;
        (
            row.0,
            row.1.filter(|s| !s.is_empty()),
            row.2.filter(|s| !s.is_empty()),
            row.3.filter(|s| !s.is_empty()),
        )
    };

    let content_hash_bytes = parse_content_hash_bytes(&content_hash_str)?;

    let downloads_dir = Path::new(&state.config.storage.downloads_dir);
    std::fs::create_dir_all(downloads_dir)
        .map_err(|e| format!("Failed to create downloads dir: {e}"))?;

    let known_filename = filename_opt
        .as_deref()
        .and_then(|f| Path::new(f).file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|f| Path::new(f).extension().map_or(false, |e| e != "bin"));
    let hash_prefix = alloy::hex::encode(&content_hash_bytes[..8]);
    let temp_filename = format!("{}.bin", hash_prefix);
    let output_path = downloads_dir.join(known_filename.as_deref().unwrap_or(&temp_filename));

    let (blobs_client, our_node_id_str) = {
        let guard = state.ensure_iroh().await?;
        let node = guard.as_ref().unwrap();
        (node.blobs_client(), node.node_id().to_string())
    };
    let content_mgr = ContentManager::new(blobs_client);

    let already_local = content_mgr.has_blob(&content_hash_bytes).await.unwrap_or(false);
    let is_self = publisher_node_id_opt.as_deref() == Some(our_node_id_str.as_str());

    if !already_local && !is_self {
        let node_id_str = publisher_node_id_opt
            .as_deref()
            .ok_or("Publisher node ID not available — cannot redownload")?;
        let node_id: iroh::NodeId = node_id_str
            .parse()
            .map_err(|e| format!("Invalid publisher node ID: {e}"))?;
        let mut node_addr = iroh::NodeAddr::from(node_id);
        if let Some(relay_url) = publisher_relay_url_opt.as_deref() {
            if let Ok(url) = relay_url.parse() {
                node_addr = node_addr.with_relay_url(url);
            }
        }
        content_mgr
            .download_from(&content_hash_bytes, node_addr)
            .await
            .map_err(|e| format!("P2P download failed: {e}"))?;
    }

    // Export (re-hashes and verifies — see content.rs)
    if !output_path.exists() {
        content_mgr
            .export_blob(&content_hash_bytes, &output_path)
            .await
            .map_err(|e| format!("Export failed: {e}"))?;
    }

    let download_path_str = output_path.to_string_lossy().into_owned();
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
    info!("Redownload complete: {}", download_path_str);
    Ok(download_path_str)
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

    // SECURITY: Verify the file is inside the downloads directory to prevent
    // opening arbitrary/malicious paths (e.g. injected .exe paths).
    let canonical = std::fs::canonicalize(&path)
        .map_err(|e| format!("Invalid file path: {e}"))?;
    let downloads_base = std::fs::canonicalize(&state.config.storage.downloads_dir)
        .map_err(|e| format!("Downloads dir error: {e}"))?;
    if !canonical.starts_with(&downloads_base) {
        return Err("Security: file is not in downloads directory".into());
    }

    opener::open(&canonical).map_err(|e| format!("Failed to open file: {e}"))?;
    Ok(path)
}

/// Open the folder containing a downloaded content file in the OS file explorer.
/// Returns the folder path that was opened.
#[tauri::command]
pub async fn open_content_folder(
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

    // SECURITY: Verify the file is inside the downloads directory
    let canonical = std::fs::canonicalize(&path)
        .map_err(|e| format!("Invalid file path: {e}"))?;
    let downloads_base = std::fs::canonicalize(&state.config.storage.downloads_dir)
        .map_err(|e| format!("Downloads dir error: {e}"))?;
    if !canonical.starts_with(&downloads_base) {
        return Err("Security: file is not in downloads directory".into());
    }

    let folder = canonical
        .parent()
        .ok_or("Could not determine parent folder")?;
    opener::open(folder).map_err(|e| format!("Failed to open folder: {e}"))?;
    Ok(folder.to_string_lossy().into_owned())
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

/// Store a buyer-signed delivery receipt and broadcast it on the gossip network.
/// Called after the buyer signs the EIP-712 receipt in the frontend.
#[tauri::command]
pub async fn broadcast_delivery_receipt(
    state: State<'_, AppState>,
    content_id: String,
    seeder_eth_address: String,
    buyer_eth_address: String,
    signature: String,
    timestamp: u64,
    bytes_served: u64,
) -> Result<(), String> {
    info!(
        "Broadcasting delivery receipt: content={}, seeder={}, buyer={}, bytes={}",
        content_id, seeder_eth_address, buyer_eth_address, bytes_served
    );

    // Store in DB
    {
        let db = state.db.lock().await;
        db.insert_delivery_receipt(
            &content_id,
            &seeder_eth_address,
            &buyer_eth_address,
            &signature,
            timestamp as i64,
            bytes_served,
        )
        .map_err(|e| format!("DB insert failed: {e}"))?;
    }

    // Look up BLAKE3 content hash (gossip topic key) from content table
    let content_hash_str: Option<String> = {
        let db = state.db.lock().await;
        db.conn()
            .query_row(
                "SELECT content_hash FROM content WHERE content_id = ?1",
                rusqlite::params![&content_id],
                |row| row.get(0),
            )
            .ok()
    };

    let Some(hash_str) = content_hash_str else {
        // Content not in local DB — still recorded, just can't broadcast right now
        return Ok(());
    };

    let content_hash = parse_content_hash_bytes(&hash_str)?;
    let content_id_bytes = parse_32byte_hex(&content_id, "content ID")?;
    let seeder_bytes = parse_20byte_hex(&seeder_eth_address, "seeder address")?;
    let buyer_bytes = parse_20byte_hex(&buyer_eth_address, "buyer address")?;
    let sig_bytes = parse_65byte_hex(&signature, "signature")?;

    state
        .send_gossip(crate::gossip_actor::GossipCmd::BroadcastDeliveryReceipt {
            content_hash,
            content_id: content_id_bytes,
            seeder_eth_address: seeder_bytes,
            buyer_eth_address: buyer_bytes,
            signature: sig_bytes,
            timestamp,
            bytes_served,
        })
        .await?;

    info!("Delivery receipt stored and broadcast for {}", content_id);
    Ok(())
}

/// Return the configured marketplace contract address (needed by the frontend for EIP-712 domain).
#[tauri::command]
pub async fn get_marketplace_address(
    state: State<'_, AppState>,
) -> Result<String, String> {
    Ok(state.config.ethereum.marketplace_address.clone())
}

/// Build a tip transaction for the frontend to sign.
/// Tips use the same 85/2.5/12.5 split as purchases but don't mint editions.
#[tauri::command]
pub async fn tip_content(
    state: State<'_, AppState>,
    content_id: String,
    tip_amount_eth: String,
) -> Result<Vec<TransactionRequest>, String> {
    info!("Preparing tip for content: {}, amount: {} ETH", content_id, tip_amount_eth);

    let tip_wei = crate::commands::types::parse_token_amount(&tip_amount_eth)?;
    if tip_wei.is_zero() {
        return Err("Tip amount must be greater than 0".to_string());
    }

    let marketplace_addr: Address = state
        .config
        .ethereum
        .marketplace_address
        .parse()
        .map_err(|e| format!("Invalid marketplace address: {e}"))?;

    let content_id_bytes: FixedBytes<32> = content_id
        .strip_prefix("0x")
        .unwrap_or(&content_id)
        .parse()
        .map_err(|e| format!("Invalid content ID: {e}"))?;

    let calldata = MarketplaceClient::<()>::tip_content_calldata(content_id_bytes);

    Ok(vec![TransactionRequest {
        to: format!("{marketplace_addr:#x}"),
        data: hex_encode(&calldata),
        value: format!("0x{:x}", tip_wei),
        description: format!("Tip {} ETH to content creator", tip_amount_eth),
    }])
}

/// Confirm a tip transaction. The frontend calls this after the tx is confirmed on-chain.
/// Records the tip in the local DB so it shows up in Transaction History.
#[tauri::command]
pub async fn confirm_tip(
    state: State<'_, AppState>,
    content_id: String,
    tx_hash: String,
    tip_amount_eth: String,
) -> Result<(), String> {
    info!("Tip confirmed: content={}, tx={}, amount={} ETH", content_id, tx_hash, tip_amount_eth);

    let tipper = state.wallet_address.lock().await.as_ref().cloned();
    let tip_wei = crate::commands::types::parse_token_amount(&tip_amount_eth)
        .unwrap_or(alloy::primitives::U256::ZERO);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    if let Some(addr) = tipper {
        let db = state.db.lock().await;
        let _ = db.conn().execute(
            "INSERT OR REPLACE INTO tips_sent (tx_hash, content_id, tipper, amount_wei, tipped_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![&tx_hash, &content_id, &addr, &tip_wei.to_string(), now],
        );
    }
    Ok(())
}

/// Get the count of delivery receipts for a content item.
#[tauri::command]
pub async fn get_receipt_count(
    state: State<'_, AppState>,
    content_id: String,
) -> Result<u64, String> {
    let db = state.db.lock().await;
    let count: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM delivery_receipts WHERE content_id = ?1",
            rusqlite::params![&content_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    Ok(count as u64)
}

// ── Resale marketplace ──

#[derive(Serialize, Deserialize)]
pub struct ResaleListing {
    pub content_id: String,
    pub seller: String,
    pub price_eth: String,
    pub listed_at: i64,
}

#[derive(Serialize, Deserialize)]
pub struct EditionInfo {
    pub max_supply: u64,
    pub total_minted: u64,
    pub royalty_bps: u32,
}

#[derive(Serialize, Deserialize)]
pub struct BuyResalePrepareResult {
    pub content_id: String,
    pub title: String,
    pub price_eth: String,
    pub transactions: Vec<TransactionRequest>,
}

/// Prepare transactions for listing a purchased content item for resale.
/// May return 1 or 2 transactions: setApprovalForAll (if needed) + listForResale.
#[tauri::command]
pub async fn list_for_resale(
    state: State<'_, AppState>,
    content_id: String,
    price_eth: String,
) -> Result<Vec<TransactionRequest>, String> {
    info!("Preparing list-for-resale: content={}, price={}", content_id, price_eth);

    let wallet = state.wallet_address.lock().await;
    let seller_str = wallet.as_ref().ok_or("No wallet connected")?.clone();
    drop(wallet);

    let seller_addr: Address = seller_str
        .parse()
        .map_err(|e| format!("Invalid wallet address: {e}"))?;

    let content_id_bytes: FixedBytes<32> = content_id
        .strip_prefix("0x")
        .unwrap_or(&content_id)
        .parse()
        .map_err(|e| format!("Invalid content ID: {e}"))?;

    let price_wei = parse_token_amount(&price_eth)
        .map_err(|e| format!("Invalid price: {e}"))?;

    let marketplace_addr: Address = state.config.ethereum.marketplace_address
        .parse()
        .map_err(|e| format!("Invalid marketplace address: {e}"))?;

    let registry_addr: Address = state.config.ethereum.registry_address
        .parse()
        .map_err(|e| format!("Invalid registry address: {e}"))?;

    let mut transactions = Vec::new();

    // Check if seller has approved marketplace for ERC-1155 transfers
    let chain = state.chain_client().map_err(|e| format!("Chain client error: {e}"))?;
    let approved = chain.registry
        .is_approved_for_all(seller_addr, marketplace_addr)
        .await
        .unwrap_or(false);

    if !approved {
        let calldata = ContentTokenClient::<()>::set_approval_for_all_calldata(marketplace_addr, true);
        transactions.push(TransactionRequest {
            to: format!("{registry_addr:#x}"),
            data: hex_encode(&calldata),
            value: "0x0".to_string(),
            description: "Approve marketplace for token transfers".to_string(),
        });
    }

    // Build listForResale transaction
    let calldata = MarketplaceClient::<()>::list_for_resale_calldata(content_id_bytes, price_wei);
    transactions.push(TransactionRequest {
        to: format!("{marketplace_addr:#x}"),
        data: hex_encode(&calldata),
        value: "0x0".to_string(),
        description: format!("List for resale at {} ETH", price_eth),
    });

    Ok(transactions)
}

/// Record a resale listing in local DB after the transaction is confirmed.
#[tauri::command]
pub async fn confirm_list_for_resale(
    state: State<'_, AppState>,
    content_id: String,
    price_eth: String,
) -> Result<(), String> {
    let wallet = state.wallet_address.lock().await;
    let seller = wallet.as_ref().ok_or("No wallet connected")?.clone();
    drop(wallet);

    let price_wei = parse_token_amount(&price_eth)
        .map_err(|e| format!("Invalid price: {e}"))?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let db = state.db.lock().await;
    db.upsert_resale_listing(&content_id, &seller, &price_wei.to_string(), now)
        .map_err(|e| format!("DB insert failed: {e}"))?;

    info!("Resale listing confirmed: content={}, seller={}, price={}", content_id, seller, price_eth);
    Ok(())
}

/// Prepare a transaction to cancel a resale listing.
#[tauri::command]
pub async fn cancel_resale_listing(
    state: State<'_, AppState>,
    content_id: String,
) -> Result<Vec<TransactionRequest>, String> {
    let wallet = state.wallet_address.lock().await;
    let _seller = wallet.as_ref().ok_or("No wallet connected")?.clone();
    drop(wallet);

    let content_id_bytes: FixedBytes<32> = content_id
        .strip_prefix("0x")
        .unwrap_or(&content_id)
        .parse()
        .map_err(|e| format!("Invalid content ID: {e}"))?;

    let marketplace_addr: Address = state.config.ethereum.marketplace_address
        .parse()
        .map_err(|e| format!("Invalid marketplace address: {e}"))?;

    let calldata = MarketplaceClient::<()>::cancel_listing_calldata(content_id_bytes);

    Ok(vec![TransactionRequest {
        to: format!("{marketplace_addr:#x}"),
        data: hex_encode(&calldata),
        value: "0x0".to_string(),
        description: "Cancel resale listing".to_string(),
    }])
}

/// Record cancellation of a resale listing in local DB.
#[tauri::command]
pub async fn confirm_cancel_listing(
    state: State<'_, AppState>,
    content_id: String,
) -> Result<(), String> {
    let wallet = state.wallet_address.lock().await;
    let seller = wallet.as_ref().ok_or("No wallet connected")?.clone();
    drop(wallet);

    let db = state.db.lock().await;
    db.deactivate_resale_listing(&content_id, &seller)
        .map_err(|e| format!("DB update failed: {e}"))?;

    info!("Resale listing cancelled: content={}, seller={}", content_id, seller);
    Ok(())
}

/// Prepare a transaction to buy content from a reseller.
/// After signing, the frontend reuses `confirm_purchase` for download + seeding.
#[tauri::command]
pub async fn buy_resale(
    state: State<'_, AppState>,
    content_id: String,
    seller: String,
) -> Result<BuyResalePrepareResult, String> {
    info!("Preparing resale purchase: content={}, seller={}", content_id, seller);

    let wallet = state.wallet_address.lock().await;
    let _buyer = wallet.as_ref().ok_or("No wallet connected")?.clone();
    drop(wallet);

    // Look up content title from DB
    let title = {
        let db = state.db.lock().await;
        db.conn()
            .query_row(
                "SELECT title FROM content WHERE content_id = ?1",
                rusqlite::params![&content_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .map_err(|e| format!("Content not found: {e}"))?
            .unwrap_or_default()
    };

    let content_id_bytes: FixedBytes<32> = content_id
        .strip_prefix("0x")
        .unwrap_or(&content_id)
        .parse()
        .map_err(|e| format!("Invalid content ID: {e}"))?;

    let seller_addr: Address = seller
        .parse()
        .map_err(|e| format!("Invalid seller address: {e}"))?;

    let marketplace_addr: Address = state.config.ethereum.marketplace_address
        .parse()
        .map_err(|e| format!("Invalid marketplace address: {e}"))?;

    // Read listing price from on-chain (source of truth) — not from local DB cache
    let chain = state.chain_client().map_err(|e| format!("Chain client error: {e}"))?;
    let (price_wei, active) = chain.marketplace
        .get_listing(content_id_bytes, seller_addr)
        .await
        .map_err(|e| format!("Failed to read listing on-chain: {e}"))?;

    if !active {
        return Err("This listing is no longer active on-chain".to_string());
    }
    if price_wei.is_zero() {
        return Err("No listing found on-chain for this seller".to_string());
    }
    let price_eth = format_wei(price_wei);

    let calldata = MarketplaceClient::<()>::buy_resale_calldata(content_id_bytes, seller_addr, price_wei);
    let value_hex = format!("0x{:x}", price_wei);

    Ok(BuyResalePrepareResult {
        content_id: content_id.clone(),
        title: title.clone(),
        price_eth: price_eth.clone(),
        transactions: vec![TransactionRequest {
            to: format!("{marketplace_addr:#x}"),
            data: hex_encode(&calldata),
            value: value_hex,
            description: format!("Buy \"{}\" (resale) for {} ETH", title, price_eth),
        }],
    })
}

/// Get all active resale listings for a content item.
#[tauri::command]
pub async fn get_resale_listings(
    state: State<'_, AppState>,
    content_id: String,
) -> Result<Vec<ResaleListing>, String> {
    let db = state.db.lock().await;
    let rows = db.get_active_resale_listings(&content_id)
        .map_err(|e| format!("DB query failed: {e}"))?;

    let listings = rows
        .into_iter()
        .map(|(cid, seller, price_wei_str, listed_at)| {
            let price_wei: U256 = price_wei_str.parse().unwrap_or(U256::ZERO);
            ResaleListing {
                content_id: cid,
                seller,
                price_eth: format_wei(price_wei),
                listed_at,
            }
        })
        .collect();

    Ok(listings)
}

/// Get on-chain edition info for a content item (max supply, total minted, royalty).
#[tauri::command]
pub async fn get_edition_info(
    state: State<'_, AppState>,
    content_id: String,
) -> Result<EditionInfo, String> {
    let content_id_bytes: FixedBytes<32> = content_id
        .strip_prefix("0x")
        .unwrap_or(&content_id)
        .parse()
        .map_err(|e| format!("Invalid content ID: {e}"))?;

    let chain = state.chain_client().map_err(|e| format!("Chain client error: {e}"))?;

    let max_supply = chain.registry
        .get_max_supply(content_id_bytes)
        .await
        .unwrap_or(U256::ZERO);

    let total_minted = chain.registry
        .get_total_minted(content_id_bytes)
        .await
        .unwrap_or(U256::ZERO);

    // Read royalty_bps from DB (cheaper than on-chain query)
    let royalty_bps: u32 = {
        let db = state.db.lock().await;
        db.conn()
            .query_row(
                "SELECT royalty_bps FROM content WHERE content_id = ?1",
                rusqlite::params![&content_id],
                |row| row.get::<_, i32>(0),
            )
            .unwrap_or(0) as u32
    };

    Ok(EditionInfo {
        max_supply: max_supply.try_into().unwrap_or(u64::MAX),
        total_minted: total_minted.try_into().unwrap_or(u64::MAX),
        royalty_bps,
    })
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

fn parse_32byte_hex(s: &str, label: &str) -> Result<[u8; 32], String> {
    let hex_str = s.strip_prefix("0x").unwrap_or(s);
    let bytes = alloy::hex::decode(hex_str).map_err(|e| format!("Invalid {label}: {e}"))?;
    if bytes.len() != 32 {
        return Err(format!("{label} must be 32 bytes, got {}", bytes.len()));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn parse_20byte_hex(s: &str, label: &str) -> Result<[u8; 20], String> {
    let hex_str = s.strip_prefix("0x").unwrap_or(s);
    let bytes = alloy::hex::decode(hex_str).map_err(|e| format!("Invalid {label}: {e}"))?;
    if bytes.len() != 20 {
        return Err(format!("{label} must be 20 bytes, got {}", bytes.len()));
    }
    let mut out = [0u8; 20];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn parse_65byte_hex(s: &str, label: &str) -> Result<Vec<u8>, String> {
    let hex_str = s.strip_prefix("0x").unwrap_or(s);
    let bytes = alloy::hex::decode(hex_str).map_err(|e| format!("Invalid {label}: {e}"))?;
    if bytes.len() != 65 {
        return Err(format!("{label} must be 65 bytes, got {}", bytes.len()));
    }
    Ok(bytes)
}
