use crate::commands::types::{format_token_amount, format_wei, hex_encode, parse_token_amount, parse_token_amount_with_decimals, TransactionRequest};
use crate::gossip_actor::GossipCmd;
use crate::state::AppState;
use alloy::primitives::{Address, FixedBytes, TxHash, U256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::sol_types::SolEvent;
use ara_chain::contracts::IAraContent;
use ara_chain::content_token::ContentTokenClient;
use ara_p2p::content::ContentManager;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use tauri::{Emitter, State};
use tracing::{info, warn};

/// Content the connected wallet has published (creator = wallet).
/// Includes live seeding status for the Library Published tab.
#[derive(Serialize, Deserialize)]
pub struct PublishedItem {
    pub content_id: String,
    pub title: String,
    pub content_type: String,
    /// Formatted price string (e.g. "0.10" for USDC, "0.001" for ETH)
    pub price_display: String,
    /// Currency symbol (e.g. "ETH", "USDC")
    pub price_symbol: String,
    pub is_seeding: bool,
    pub file_size_bytes: i64,
    pub updated_at: Option<i64>,
    /// Arweave/Irys gateway URL if permanently stored, null otherwise
    pub arweave_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
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
    pub max_supply: i64,
    pub total_minted: i64,
    pub resale_count: i64,
    pub min_resale_price_eth: Option<String>,
    /// ERC-20 payment token address (empty string or null = ETH)
    pub payment_token: Option<String>,
    /// Token symbol (e.g. "USDC") if token-priced, otherwise "ETH"
    pub payment_token_symbol: String,
    /// Collaborator revenue splits (empty if solo creator)
    pub collaborators: Vec<CollaboratorDisplay>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CollaboratorDisplay {
    pub wallet: String,
    pub share_bps: u32,
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

/// Locate the ffmpeg (or ffprobe) binary.
/// Search order: sidecar (next to exe), system PATH, common install locations.
fn find_ffmpeg_binary(name: &str) -> Option<PathBuf> {
    let exe_name = if cfg!(windows) {
        format!("{}.exe", name)
    } else {
        name.to_string()
    };

    // 1. Bundled sidecar: binary next to our own executable (production installs)
    if let Ok(exe_dir) = std::env::current_exe().map(|p| p.parent().unwrap_or(Path::new(".")).to_path_buf()) {
        let sidecar = exe_dir.join(&exe_name);
        if sidecar.exists() {
            info!("Found bundled {}: {}", name, sidecar.display());
            return Some(sidecar);
        }
    }

    // 2. System PATH fallback — only in debug/development builds.
    // SECURITY: In release builds, never fall back to system PATH to prevent
    // trojan ffmpeg binaries from executing with user privileges.
    #[cfg(debug_assertions)]
    {
        let check = if cfg!(windows) {
            Command::new("where").arg(&exe_name).output()
        } else {
            Command::new("which").arg(&exe_name).output()
        };

        if let Ok(output) = check {
            if output.status.success() {
                let path_str = String::from_utf8_lossy(&output.stdout);
                let first_line = path_str.lines().next().unwrap_or("").trim();
                if !first_line.is_empty() {
                    info!("Found {} in PATH: {} (debug build only)", name, first_line);
                    return Some(PathBuf::from(first_line));
                }
            }
        }
    }

    warn!("{} not found (sidecar not bundled{})", name,
        if cfg!(debug_assertions) { " and not in PATH" } else { "" });
    None
}

/// Probe a video file to get its resolution and bitrate using ffprobe.
/// Returns (width, height, bitrate_bps) or an error.
fn ffprobe_video(ffprobe_path: &Path, video_path: &Path) -> Result<(u32, u32, u64), String> {
    let output = Command::new(ffprobe_path)
        .args([
            "-v", "quiet",
            "-print_format", "json",
            "-show_streams",
            "-show_format",
        ])
        .arg(video_path)
        .output()
        .map_err(|e| format!("ffprobe failed to execute: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("ffprobe error: {stderr}"));
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("ffprobe JSON parse error: {e}"))?;

    // Find the video stream
    let mut width = 0u32;
    let mut height = 0u32;

    if let Some(streams) = json.get("streams").and_then(|s| s.as_array()) {
        for stream in streams {
            if stream.get("codec_type").and_then(|t| t.as_str()) == Some("video") {
                width = stream.get("width").and_then(|w| w.as_u64()).unwrap_or(0) as u32;
                height = stream.get("height").and_then(|h| h.as_u64()).unwrap_or(0) as u32;
                break;
            }
        }
    }

    // Get overall bitrate from format
    let bitrate = json
        .get("format")
        .and_then(|f| f.get("bit_rate"))
        .and_then(|b| b.as_str())
        .and_then(|b| b.parse::<u64>().ok())
        .unwrap_or(0);

    Ok((width, height, bitrate))
}

/// Transcode a video to 1080p max 5Mbps H.264 MP4 if it exceeds those limits.
/// Returns the path to use (original if compliant, temp transcoded file if not).
fn transcode_preview_video(
    ffmpeg_path: &Path,
    ffprobe_path: &Path,
    input_path: &Path,
) -> Result<PathBuf, String> {
    let (width, height, bitrate) = ffprobe_video(ffprobe_path, input_path)?;

    let max_bitrate: u64 = 5_000_000; // 5 Mbps
    let max_height: u32 = 1080;
    let max_width: u32 = 1920;

    let needs_resize = width > max_width || height > max_height;
    let needs_bitrate_cap = bitrate > max_bitrate;

    if !needs_resize && !needs_bitrate_cap {
        info!(
            "Video {}x{} @ {}bps is within limits, skipping transcode",
            width, height, bitrate
        );
        return Ok(input_path.to_path_buf());
    }

    info!(
        "Transcoding video {}x{} @ {}bps -> max {}x{} @ {}bps",
        width, height, bitrate, max_width, max_height, max_bitrate
    );

    // Build output path in same directory with _transcoded suffix
    let stem = input_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "video".to_string());
    let output_path = input_path
        .parent()
        .unwrap_or(Path::new("."))
        .join(format!("{}_transcoded.mp4", stem));

    let mut args = vec![
        "-i".to_string(),
        input_path.to_string_lossy().into_owned(),
        "-y".to_string(), // overwrite output
    ];

    // Video filter for scaling (preserve aspect ratio)
    if needs_resize {
        args.extend([
            "-vf".to_string(),
            "scale='min(1920,iw)':'min(1080,ih)':force_original_aspect_ratio=decrease:force_divisible_by=2".to_string(),
        ]);
    }

    // H.264 encoding with bitrate cap
    args.extend([
        "-c:v".to_string(), "libx264".to_string(),
        "-preset".to_string(), "medium".to_string(),
        "-b:v".to_string(), "5M".to_string(),
        "-maxrate".to_string(), "5M".to_string(),
        "-bufsize".to_string(), "10M".to_string(),
        "-c:a".to_string(), "aac".to_string(),
        "-b:a".to_string(), "128k".to_string(),
        "-movflags".to_string(), "+faststart".to_string(),
    ]);

    args.push(output_path.to_string_lossy().into_owned());

    let output = Command::new(ffmpeg_path)
        .args(&args)
        .output()
        .map_err(|e| format!("ffmpeg failed to execute: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("ffmpeg transcode failed: {stderr}"));
    }

    info!("Transcode complete: {}", output_path.display());
    Ok(output_path)
}

/// Import a list of preview file paths into iroh and return PreviewAsset records.
/// Video files are automatically transcoded to 1080p/5Mbps H.264 if they exceed those limits.
async fn import_preview_files(
    content_mgr: &ContentManager,
    file_paths: &[String],
) -> Result<Vec<PreviewAsset>, String> {
    // Locate ffmpeg/ffprobe once for all files
    let ffmpeg = find_ffmpeg_binary("ffmpeg");
    let ffprobe = find_ffmpeg_binary("ffprobe");

    let mut assets = Vec::new();
    let mut temp_files: Vec<PathBuf> = Vec::new();

    for path_str in file_paths {
        let p = Path::new(path_str);
        if !p.exists() {
            return Err(format!("Preview file not found: {}", path_str));
        }

        // SECURITY: Reject oversized preview files to prevent disk/memory exhaustion.
        let file_meta = std::fs::metadata(p)
            .map_err(|e| format!("Cannot read preview file metadata: {e}"))?;
        let max_preview_size: u64 = if preview_asset_type(path_str) == "video" {
            500 * 1024 * 1024 // 500 MB for video previews
        } else {
            50 * 1024 * 1024 // 50 MB for image previews
        };
        if file_meta.len() > max_preview_size {
            return Err(format!(
                "Preview file too large: {} bytes (max {} MB)",
                file_meta.len(),
                max_preview_size / (1024 * 1024)
            ));
        }

        // Transcode video previews if ffmpeg is available
        let import_path = if preview_asset_type(path_str) == "video" {
            if let (Some(ref ff), Some(ref fp)) = (&ffmpeg, &ffprobe) {
                match transcode_preview_video(ff, fp, p) {
                    Ok(transcoded) => {
                        if transcoded != p {
                            info!("Using transcoded preview: {}", transcoded.display());
                            temp_files.push(transcoded.clone());
                        }
                        transcoded
                    }
                    Err(e) => {
                        warn!("Transcode failed, using original: {e}");
                        p.to_path_buf()
                    }
                }
            } else {
                warn!("ffmpeg not found, importing video preview without transcoding");
                p.to_path_buf()
            }
        } else {
            p.to_path_buf()
        };

        let hash_bytes = content_mgr
            .import_file(&import_path)
            .await
            .map_err(|e| format!("Failed to import preview {}: {e}", path_str))?;

        let size = std::fs::metadata(&import_path).map(|m| m.len()).unwrap_or(0);

        // Use original filename for metadata (not _transcoded suffix)
        let filename = p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        // For transcoded videos, ensure the asset_type and extension reflect MP4
        let asset_type = preview_asset_type(path_str).to_string();

        assets.push(PreviewAsset {
            asset_type,
            hash: format!("0x{}", alloy::hex::encode(hash_bytes)),
            filename,
            size,
        });
    }

    // Clean up temporary transcoded files
    for temp in &temp_files {
        if let Err(e) = std::fs::remove_file(temp) {
            warn!("Failed to clean up temp file {}: {e}", temp.display());
        }
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
    max_supply: Option<u64>,
    royalty_bps: Option<u32>,
    categories: Option<Vec<String>>,
    main_preview_image_path: Option<String>,
    main_preview_trailer_path: Option<String>,
    preview_paths: Option<Vec<String>>,
    payment_token: Option<String>,
    collaborators: Option<Vec<super::types::CollaboratorInput>>,
) -> Result<PublishPrepareResult, String> {
    info!(
        "Publishing content: title={}, file={}, price={}, token={:?}",
        title, file_path, price_eth, payment_token
    );

    // 1. Require wallet connected
    let wallet = state.wallet_address.lock().await;
    let creator = wallet.as_ref().ok_or("No wallet connected")?.clone();
    drop(wallet);

    // 2. Parse price — use token-specific decimals if paying in an ERC-20
    let token_decimals = if let Some(addr) = payment_token.as_ref() {
        state.config.ethereum.supported_tokens.iter()
            .find(|t| t.address.eq_ignore_ascii_case(addr))
            .map(|t| t.decimals)
            .ok_or_else(|| format!("Payment token {} is not in supported_tokens config", addr))?
    } else {
        18
    };
    let price_wei = parse_token_amount_with_decimals(&price_eth, token_decimals)?;

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
                  filename, publisher_node_id, publisher_relay_url, categories, payment_token)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, ?10, ?11, ?12, ?13, ?14, ?15)",
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
                    &payment_token.as_deref().unwrap_or(""),
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

        // Build collaborator list if provided
        let collab_list: Vec<ara_chain::contracts::IAraContent::Collaborator> = collaborators
            .as_ref()
            .map(|cs| {
                cs.iter()
                    .map(|c| {
                        let wallet: Address = c.wallet.parse().unwrap_or(Address::ZERO);
                        ara_chain::contracts::IAraContent::Collaborator {
                            wallet,
                            shareBps: U256::from(c.share_bps),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        let has_collabs = !collab_list.is_empty();

        let calldata = if has_collabs {
            // Validate collaborators
            let total: u32 = collaborators.as_ref().unwrap().iter().map(|c| c.share_bps).sum();
            if total != 10000 {
                return Err(format!("Collaborator shares must sum to 10000 (got {total})"));
            }
            if collab_list.len() > 5 {
                return Err("Maximum 5 collaborators allowed".to_string());
            }

            ContentTokenClient::<()>::publish_content_with_collaborators_calldata(
                content_hash_fixed,
                metadata_uri.clone(),
                price_wei,
                U256::from(file_size),
                U256::from(max_supply.unwrap_or(0)),
                royalty_bps.unwrap_or(1000) as u128,
                collab_list,
            )
        } else if let Some(ref token_addr_str) = payment_token {
            let token_addr: Address = token_addr_str
                .parse()
                .map_err(|e| format!("Invalid payment token address: {e}"))?;
            ContentTokenClient::<()>::publish_content_with_token_calldata(
                content_hash_fixed,
                metadata_uri.clone(),
                price_wei,
                U256::from(file_size),
                U256::from(max_supply.unwrap_or(0)),
                royalty_bps.unwrap_or(1000) as u128,
                token_addr,
            )
        } else {
            ContentTokenClient::<()>::publish_content_calldata(
                content_hash_fixed,
                metadata_uri.clone(),
                price_wei,
                U256::from(file_size),
                U256::from(max_supply.unwrap_or(0)),
                royalty_bps.unwrap_or(1000) as u128,
            )
        };

        // Find token symbol for description
        let price_unit = payment_token.as_ref().and_then(|addr| {
            state.config.ethereum.supported_tokens.iter()
                .find(|t| t.address.eq_ignore_ascii_case(addr))
                .map(|t| t.symbol.clone())
        }).unwrap_or_else(|| "ETH".to_string());

        vec![TransactionRequest {
            to: format!("{registry_addr:#x}"),
            data: hex_encode(&calldata),
            value: "0x0".to_string(),
            description: format!("Publish \"{}\" for {} {}", title, price_eth, price_unit),
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
) -> Result<String, String> {
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
    Ok(content_id_hex)
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
        if let Ok(event) = IAraContent::ContentPublished::decode_log(&log.inner) {
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

    let calldata = ContentTokenClient::<()>::update_content_calldata(
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
        ContentTokenClient::<()>::update_content_file_calldata(content_id_fixed, new_hash_fixed);

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

    let supported_tokens = &state.config.ethereum.supported_tokens;

    let mut stmt = conn
        .prepare(
            "SELECT content_id, content_hash, creator, title, description,
                    content_type, price_wei, active, file_size_bytes,
                    COALESCE(metadata_uri,''), updated_at, COALESCE(categories,'[]'),
                    COALESCE(max_supply,0), COALESCE(total_minted,0),
                    COALESCE(payment_token,'')
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
            let pt: String = row.get(14)?;
            let is_token = !pt.is_empty() && pt != "0x0000000000000000000000000000000000000000";
            let token_cfg = if is_token {
                supported_tokens.iter().find(|t| t.address.eq_ignore_ascii_case(&pt))
            } else {
                None
            };
            let symbol = token_cfg.map(|t| t.symbol.clone()).unwrap_or_else(|| "ETH".to_string());
            let decimals = token_cfg.map(|t| t.decimals).unwrap_or(18);
            Ok(ContentDetail {
                content_id: row.get(0)?,
                content_hash: row.get(1)?,
                creator: row.get(2)?,
                title: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                description: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                content_type: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                price_eth: format_token_amount(price_wei, decimals),
                active: row.get::<_, i32>(7)? != 0,
                seeder_count: 0,
                purchase_count: 0,
                metadata_uri: row.get(9)?,
                updated_at: row.get(10)?,
                categories: serde_json::from_str(&cats_json).unwrap_or_default(),
                max_supply: row.get(12)?,
                total_minted: row.get(13)?,
                resale_count: 0,
                min_resale_price_eth: None,
                payment_token: if is_token { Some(pt) } else { None },
                payment_token_symbol: symbol,
                collaborators: vec![],
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
    let calldata = ContentTokenClient::<()>::delist_content_calldata(content_id_fixed);

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

    // Look up supported token configs for symbol/decimal resolution
    let supported_tokens = &state.config.ethereum.supported_tokens;

    let mut stmt = conn
        .prepare(
            "SELECT c.content_id, c.title, c.content_type, c.price_wei,
                    COALESCE(s.active, 0) as is_seeding,
                    COALESCE(c.file_size_bytes, 0),
                    c.updated_at,
                    c.payment_token,
                    c.arweave_tx_id
             FROM content c
             LEFT JOIN seeding s ON c.content_id = s.content_id
             WHERE LOWER(c.creator) = ?1 AND c.active = 1
             ORDER BY c.created_at DESC",
        )
        .map_err(|e| format!("DB query failed: {e}"))?;

    let irys_node_url = state.config.arweave.node_url.clone();

    let rows = stmt
        .query_map(rusqlite::params![&creator], |row| {
            let price_wei_str: String = row.get(3)?;
            let price_wei = price_wei_str
                .parse::<alloy::primitives::U256>()
                .unwrap_or(alloy::primitives::U256::ZERO);
            let payment_token: Option<String> = row.get(7)?;
            let arweave_tx_id: Option<String> = row.get(8)?;

            // Determine price display and symbol based on payment token
            let (price_display, price_symbol) = match &payment_token {
                Some(addr) if !addr.is_empty() && addr != "0x0000000000000000000000000000000000000000" => {
                    // Find token config for decimals/symbol
                    let token_cfg = supported_tokens.iter().find(|t| t.address.eq_ignore_ascii_case(addr));
                    match token_cfg {
                        Some(cfg) => (format_token_amount(price_wei, cfg.decimals), cfg.symbol.clone()),
                        None => (format_wei(price_wei), "TOKEN".to_string()),
                    }
                }
                _ => (format_wei(price_wei), "ETH".to_string()),
            };

            // Build Arweave gateway URL
            let arweave_url = arweave_tx_id
                .filter(|tx| !tx.is_empty())
                .map(|tx| {
                    if irys_node_url.contains("devnet") {
                        format!("https://devnet.irys.xyz/{}", tx)
                    } else {
                        format!("https://arweave.net/{}", tx)
                    }
                });

            Ok(PublishedItem {
                content_id: row.get(0)?,
                title: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                content_type: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                price_display,
                price_symbol,
                is_seeding: row.get::<_, i32>(4)? != 0,
                file_size_bytes: row.get(5)?,
                updated_at: row.get(6)?,
                arweave_url,
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

    let supported_tokens = state.config.ethereum.supported_tokens.clone();

    // Scope the DB lock so it's dropped before the async chain call
    let mut detail = {
        let db = state.db.lock().await;
        let conn = db.conn();

        let mut stmt = conn
            .prepare(
                "SELECT content_id, content_hash, creator, title, description,
                        content_type, price_wei, active, file_size_bytes,
                        COALESCE(metadata_uri,''), updated_at, COALESCE(categories,'[]'),
                        COALESCE(max_supply,0), COALESCE(total_minted,0),
                        COALESCE(payment_token,'')
                 FROM content WHERE content_id = ?1",
            )
            .map_err(|e| format!("DB query failed: {e}"))?;

        stmt.query_row(rusqlite::params![&content_id], |row| {
            let price_wei_str: String = row.get(6)?;
            let price_wei = price_wei_str
                .parse::<alloy::primitives::U256>()
                .unwrap_or(alloy::primitives::U256::ZERO);
            let cats_json: String = row.get(11)?;
            let pt: String = row.get(14)?;
            let is_token = !pt.is_empty() && pt != "0x0000000000000000000000000000000000000000";
            let token_cfg = if is_token {
                supported_tokens.iter().find(|t| t.address.eq_ignore_ascii_case(&pt))
            } else {
                None
            };
            let symbol = token_cfg.map(|t| t.symbol.clone()).unwrap_or_else(|| "ETH".to_string());
            let decimals = token_cfg.map(|t| t.decimals).unwrap_or(18);
            Ok(ContentDetail {
                content_id: row.get(0)?,
                content_hash: row.get(1)?,
                creator: row.get(2)?,
                title: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                description: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                content_type: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                price_eth: format_token_amount(price_wei, decimals),
                active: row.get::<_, i32>(7)? != 0,
                seeder_count: 0,
                purchase_count: 0,
                metadata_uri: row.get(9)?,
                updated_at: row.get(10)?,
                categories: serde_json::from_str(&cats_json).unwrap_or_default(),
                max_supply: row.get(12)?,
                total_minted: row.get(13)?,
                resale_count: 0,
                min_resale_price_eth: None,
                payment_token: if is_token { Some(pt) } else { None },
                payment_token_symbol: symbol,
                collaborators: vec![],
            })
        })
        .map_err(|e| format!("Content not found: {e}"))?
    }; // DB lock dropped here
    if let Ok(chain) = state.chain_client() {
        if let Ok(content_id_bytes) = content_id.parse::<alloy::primitives::FixedBytes<32>>() {
            if let Ok(collabs) = chain.registry.get_collaborators(content_id_bytes).await {
                detail.collaborators = collabs
                    .into_iter()
                    .map(|c| CollaboratorDisplay {
                        wallet: format!("{:#x}", c.wallet),
                        share_bps: c.shareBps.try_into().unwrap_or(0),
                    })
                    .collect();
            }
        }
    }

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

    let supported_tokens = &state.config.ethereum.supported_tokens;

    let mut stmt = conn
        .prepare(
            "SELECT c.content_id, c.content_hash, c.creator, c.title, c.description,
                    c.content_type, c.price_wei, c.active, c.file_size_bytes,
                    COALESCE(c.metadata_uri,''), c.updated_at, COALESCE(c.categories,'[]'),
                    COALESCE(c.max_supply,0), COALESCE(c.total_minted,0),
                    COALESCE(r.cnt, 0), r.min_price,
                    COALESCE(c.payment_token,'')
             FROM content c
             LEFT JOIN (
                 SELECT content_id, COUNT(*) as cnt, MIN(CAST(price_wei AS INTEGER)) as min_price
                 FROM resale_listings WHERE active = 1 GROUP BY content_id
             ) r ON c.content_id = r.content_id
             WHERE c.active = 1
             AND (c.title LIKE ?1 OR c.description LIKE ?1
                  OR c.content_type LIKE ?1 OR COALESCE(c.categories,'') LIKE ?1)
             ORDER BY c.created_at DESC LIMIT 50",
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
            let resale_count: i64 = row.get(14)?;
            let min_resale_wei: Option<i64> = row.get(15)?;
            let min_resale_price_eth = min_resale_wei.map(|w| {
                format_wei(alloy::primitives::U256::from(w as u64))
            });
            let pt: String = row.get(16)?;
            let is_token = !pt.is_empty() && pt != "0x0000000000000000000000000000000000000000";
            let token_cfg = if is_token {
                supported_tokens.iter().find(|t| t.address.eq_ignore_ascii_case(&pt))
            } else {
                None
            };
            let symbol = token_cfg.map(|t| t.symbol.clone()).unwrap_or_else(|| "ETH".to_string());
            let decimals = token_cfg.map(|t| t.decimals).unwrap_or(18);
            Ok(ContentDetail {
                content_id: row.get(0)?,
                content_hash: row.get(1)?,
                creator: row.get(2)?,
                title: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                description: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                content_type: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                price_eth: format_token_amount(price_wei, decimals),
                active: row.get::<_, i32>(7)? != 0,
                seeder_count: 0,
                purchase_count: 0,
                metadata_uri: row.get(9)?,
                updated_at: row.get(10)?,
                categories: serde_json::from_str(&cats_json).unwrap_or_default(),
                max_supply: row.get(12)?,
                total_minted: row.get(13)?,
                resale_count,
                min_resale_price_eth,
                payment_token: if is_token { Some(pt) } else { None },
                payment_token_symbol: symbol,
                collaborators: vec![],
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

// ─── Arweave / Irys permanent storage ────────────────────────────────────────

#[derive(Serialize, Deserialize)]
pub struct ArweaveCostEstimate {
    /// Cost in wei
    pub cost_wei: String,
    /// Cost formatted as ETH string
    pub cost_eth: String,
    /// File size in bytes
    pub file_size: u64,
}

/// Result from prepare_arweave_upload — tells the frontend how much to fund.
#[derive(Serialize, Deserialize)]
pub struct ArweaveUploadPlan {
    /// Cost in wei for the Irys upload
    pub cost_wei: String,
    /// Cost formatted as ETH string
    pub cost_eth: String,
    /// File size in bytes
    pub file_size: u64,
    /// Transaction to sign: sends ETH from user to the ephemeral upload key
    pub transactions: Vec<TransactionRequest>,
}

/// Result from execute_arweave_upload.
#[derive(Serialize, Deserialize)]
pub struct ArweaveUploadResult {
    /// Arweave transaction ID
    pub arweave_tx_id: String,
    /// Gateway URL for viewing
    pub gateway_url: String,
}

/// Estimate the cost to permanently store a content file on Arweave via Irys.
#[tauri::command]
pub async fn estimate_arweave_cost(
    state: State<'_, AppState>,
    content_id: String,
) -> Result<ArweaveCostEstimate, String> {
    // Look up file size from DB
    let file_size: u64 = {
        let db = state.db.lock().await;
        db.conn()
            .query_row(
                "SELECT file_size_bytes FROM content WHERE content_id = ?1",
                rusqlite::params![&content_id],
                |row| row.get::<_, i64>(0),
            )
            .map(|s| s as u64)
            .map_err(|e| format!("Content not found: {e}"))?
    };

    let irys_config = crate::arweave::IrysConfig {
        node_url: state.config.arweave.node_url.clone(),
        gateway_url: state.config.arweave.gateway_url.clone(),
    };

    let client = crate::arweave::http_client();
    let cost_wei = crate::arweave::estimate_upload_cost(&client, &irys_config, file_size)
        .await
        .map_err(|e| format!("Failed to estimate Arweave cost: {e}"))?;

    let cost_eth = crate::arweave::format_wei_as_eth(cost_wei);

    Ok(ArweaveCostEstimate {
        cost_wei: cost_wei.to_string(),
        cost_eth,
        file_size,
    })
}

/// Prepare an Arweave upload: estimate cost and return a funding transaction.
///
/// The user signs the funding TX which sends ETH to the backend's ephemeral
/// Irys upload key. After funding, call `execute_arweave_upload` to complete.
#[tauri::command]
pub async fn prepare_arweave_upload(
    state: State<'_, AppState>,
    content_id: String,
) -> Result<ArweaveUploadPlan, String> {
    // Look up file size
    let file_size: u64 = {
        let db = state.db.lock().await;
        db.conn()
            .query_row(
                "SELECT file_size_bytes FROM content WHERE content_id = ?1",
                rusqlite::params![&content_id],
                |row| row.get::<_, i64>(0),
            )
            .map(|s| s as u64)
            .map_err(|e| format!("Content not found: {e}"))?
    };

    let irys_config = crate::arweave::IrysConfig {
        node_url: state.config.arweave.node_url.clone(),
        gateway_url: state.config.arweave.gateway_url.clone(),
    };

    // Estimate Irys cost
    let client = crate::arweave::http_client();
    let cost_wei = crate::arweave::estimate_upload_cost(&client, &irys_config, file_size)
        .await
        .map_err(|e| format!("Failed to estimate Arweave cost: {e}"))?;
    let cost_eth = crate::arweave::format_wei_as_eth(cost_wei);

    // Get or generate the ephemeral upload key
    let irys_key = {
        let db = state.db.lock().await;
        crate::arweave::load_or_generate_irys_key(&db)
            .map_err(|e| format!("Failed to load Irys key: {e}"))?
    };

    // Add gas margin for the ephemeral key to forward ETH to Irys deposit
    // 21000 gas * ~50 gwei = ~0.00105 ETH. Use 0.002 ETH margin for safety.
    let gas_margin: u64 = 2_000_000_000_000_000; // 0.002 ETH
    let total_needed = cost_wei.saturating_add(gas_margin);

    let ephemeral_address = format!("{:#x}", irys_key.address());

    let description = format!("Fund Arweave permanent storage ({} ETH + gas)", cost_eth);

    Ok(ArweaveUploadPlan {
        cost_wei: cost_wei.to_string(),
        cost_eth,
        file_size,
        transactions: vec![TransactionRequest {
            to: ephemeral_address,
            data: "0x".to_string(),
            value: format!("0x{:x}", total_needed),
            description,
        }],
    })
}

/// Execute the Arweave upload after the user has funded the ephemeral key.
///
/// Emits `arweave-progress` events so the frontend can show step-by-step status.
/// Steps: funding-confirm → irys-deposit → balance-credit → reading-file → uploading → done
#[tauri::command]
pub async fn execute_arweave_upload(
    state: State<'_, AppState>,
    content_id: String,
    fund_tx_hash: String,
) -> Result<ArweaveUploadResult, String> {
    let irys_config = crate::arweave::IrysConfig {
        node_url: state.config.arweave.node_url.clone(),
        gateway_url: state.config.arweave.gateway_url.clone(),
    };
    let rpc_url = state.config.ethereum.rpc_url.clone();
    let app = state.app_handle.clone();

    // Helper to emit progress events to the frontend
    let emit_progress = |step: &str, detail: &str| {
        let _ = app.emit("arweave-progress", serde_json::json!({
            "step": step,
            "detail": detail,
        }));
    };

    // Load ephemeral key
    let irys_key = {
        let db = state.db.lock().await;
        crate::arweave::load_or_generate_irys_key(&db)
            .map_err(|e| format!("Failed to load Irys key: {e}"))?
    };

    let client = crate::arweave::http_client_large_transfer();

    // Step 1: Wait for the user's funding TX to confirm on Ethereum
    emit_progress("funding-confirm", "Waiting for funding transaction to confirm...");
    info!("Waiting for funding TX: {}", fund_tx_hash);
    let tx_hash: alloy::primitives::TxHash = fund_tx_hash
        .parse()
        .map_err(|e| format!("Invalid TX hash: {e}"))?;
    let parsed_rpc = rpc_url.parse().map_err(|e| format!("Invalid RPC URL: {e}"))?;
    let provider = ProviderBuilder::new().connect_http(parsed_rpc);
    for attempt in 1..=60 {
        if let Ok(Some(receipt)) = provider.get_transaction_receipt(tx_hash).await {
            if receipt.status() {
                info!("Funding TX confirmed at attempt {}", attempt);
                break;
            }
            return Err("Funding TX reverted".to_string());
        }
        if attempt == 60 {
            return Err("Funding TX not confirmed after 5 minutes".to_string());
        }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }

    // Step 2: Forward ETH from ephemeral key to Irys deposit
    emit_progress("irys-deposit", "Depositing ETH to Irys storage network...");
    let irys_deposit = crate::arweave::get_irys_deposit_address(&client, &irys_config)
        .await
        .map_err(|e| format!("Failed to get Irys deposit address: {e}"))?;

    let file_size: u64 = {
        let db = state.db.lock().await;
        db.conn()
            .query_row(
                "SELECT file_size_bytes FROM content WHERE content_id = ?1",
                rusqlite::params![&content_id],
                |row| row.get::<_, i64>(0),
            )
            .map(|s| s as u64)
            .map_err(|e| format!("Content not found: {e}"))?
    };

    let cost_wei = crate::arweave::estimate_upload_cost(&client, &irys_config, file_size)
        .await
        .map_err(|e| format!("Cost estimate failed: {e}"))?;

    info!(
        "Forwarding {} wei from {} to Irys deposit {}",
        cost_wei,
        irys_key.address(),
        irys_deposit
    );
    let irys_fund_tx = crate::arweave::fund_irys_from_key(&rpc_url, &irys_key, &irys_deposit, cost_wei)
        .await
        .map_err(|e| format!("Irys funding failed: {e}"))?;

    // Step 3: Notify Irys about the deposit, then wait for balance credit
    emit_progress("balance-credit", "Notifying Irys of deposit...");
    // POST the funding TX hash so Irys credits the balance promptly
    crate::arweave::notify_irys_of_deposit(&client, &irys_config, &irys_fund_tx)
        .await
        .map_err(|e| format!("Irys deposit notification failed: {e}"))?;

    emit_progress("balance-credit", "Waiting for Irys to credit balance...");
    let ephemeral_addr = format!("{:#x}", irys_key.address());
    crate::arweave::wait_for_irys_balance(&client, &irys_config, &ephemeral_addr, cost_wei, 24)
        .await
        .map_err(|e| format!("Irys balance not credited: {e}"))?;

    // Step 4: Read the content file from iroh
    emit_progress("reading-file", "Reading content from P2P network...");
    let content_hash: String = {
        let db = state.db.lock().await;
        db.conn()
            .query_row(
                "SELECT content_hash FROM content WHERE content_id = ?1",
                rusqlite::params![&content_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("Content not found: {e}"))?
    };

    let hash_bytes = parse_content_hash_bytes(&content_hash)?;
    let iroh_hash = iroh_blobs::Hash::from_bytes(hash_bytes);

    let file_bytes = {
        let guard = state.ensure_iroh().await?;
        let node = guard.as_ref().unwrap();
        let blobs = node.blobs_client();
        blobs
            .read_to_bytes(iroh_hash)
            .await
            .map_err(|e| format!("Failed to read content from iroh: {e}"))?
    };

    info!("Read {} bytes from iroh for Arweave upload", file_bytes.len());

    // Detect content type from file bytes
    let content_type = infer::get(&file_bytes)
        .map(|t| t.mime_type())
        .unwrap_or("application/octet-stream");

    // Step 5: Create signed data item and upload
    let size_label = if file_bytes.len() < 1024 {
        format!("{} B", file_bytes.len())
    } else if file_bytes.len() < 1024 * 1024 {
        format!("{:.1} KB", file_bytes.len() as f64 / 1024.0)
    } else {
        format!("{:.1} MB", file_bytes.len() as f64 / (1024.0 * 1024.0))
    };
    emit_progress("uploading", &format!("Uploading {} to Arweave...", size_label));

    let signed_item = crate::arweave::create_signed_data_item(&file_bytes, content_type, &irys_key)
        .await
        .map_err(|e| format!("Failed to create data item: {e}"))?;

    let arweave_tx_id = crate::arweave::upload_data_item(&client, &irys_config, &signed_item)
        .await
        .map_err(|e| format!("Irys upload failed: {e}"))?;

    emit_progress("done", &format!("Stored on Arweave: {}", &arweave_tx_id[..12.min(arweave_tx_id.len())]));

    // Store TX ID in DB
    {
        let db = state.db.lock().await;
        db.conn()
            .execute(
                "UPDATE content SET arweave_tx_id = ?1 WHERE content_id = ?2",
                rusqlite::params![&arweave_tx_id, &content_id],
            )
            .map_err(|e| format!("DB update failed: {e}"))?;

        // Update metadata_uri to v3 format
        let metadata_uri: Option<String> = db
            .conn()
            .query_row(
                "SELECT metadata_uri FROM content WHERE content_id = ?1",
                rusqlite::params![&content_id],
                |row| row.get(0),
            )
            .ok();

        if let Some(uri) = metadata_uri {
            if let Ok(mut meta) = serde_json::from_str::<serde_json::Value>(&uri) {
                meta["v"] = serde_json::json!(3);
                meta["arweave_tx"] = serde_json::json!(&arweave_tx_id);
                // Devnet data lives on Irys devnet gateway, not arweave.net
                let gateway = if irys_config.node_url.contains("devnet") {
                    &irys_config.node_url
                } else {
                    &irys_config.gateway_url
                };
                meta["arweave_gateway"] = serde_json::json!(gateway);
                if let Ok(updated) = serde_json::to_string(&meta) {
                    let _ = db.conn().execute(
                        "UPDATE content SET metadata_uri = ?1 WHERE content_id = ?2",
                        rusqlite::params![&updated, &content_id],
                    );
                }
            }
        }
    }

    info!(
        "Arweave upload complete for {}: TX {}",
        content_id, arweave_tx_id
    );

    Ok(ArweaveUploadResult {
        arweave_tx_id,
        gateway_url: format!("{}/{}", irys_config.gateway_url, ""),
    })
}

/// Store the Arweave transaction ID in the local DB after a successful upload.
#[tauri::command]
pub async fn confirm_arweave_upload(
    state: State<'_, AppState>,
    content_id: String,
    arweave_tx_id: String,
) -> Result<(), String> {
    info!(
        "Storing Arweave TX {} for content {}",
        arweave_tx_id, content_id
    );

    let db = state.db.lock().await;
    db.conn()
        .execute(
            "UPDATE content SET arweave_tx_id = ?1 WHERE content_id = ?2",
            rusqlite::params![&arweave_tx_id, &content_id],
        )
        .map_err(|e| format!("DB update failed: {e}"))?;

    // Update metadata_uri to v3 format with arweave_tx
    let metadata_uri: Option<String> = db
        .conn()
        .query_row(
            "SELECT metadata_uri FROM content WHERE content_id = ?1",
            rusqlite::params![&content_id],
            |row| row.get(0),
        )
        .ok();

    if let Some(uri) = metadata_uri {
        if let Ok(mut meta) = serde_json::from_str::<serde_json::Value>(&uri) {
            meta["v"] = serde_json::json!(3);
            meta["arweave_tx"] = serde_json::json!(arweave_tx_id);
            meta["arweave_gateway"] = serde_json::json!(state.config.arweave.gateway_url);
            if let Ok(updated) = serde_json::to_string(&meta) {
                let _ = db.conn().execute(
                    "UPDATE content SET metadata_uri = ?1 WHERE content_id = ?2",
                    rusqlite::params![&updated, &content_id],
                );
            }
        }
    }

    Ok(())
}

/// Get the Arweave configuration (gateway URL, node URL) for the frontend.
#[tauri::command]
pub async fn get_arweave_config(
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "node_url": state.config.arweave.node_url,
        "gateway_url": state.config.arweave.gateway_url,
    }))
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
