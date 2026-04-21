mod arweave;
mod blob_events;
mod commands;
mod gossip_actor;
mod setup;
mod state;

use tauri::{Emitter, Manager};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Percent-decode a URL path component. Handles ASCII byte sequences produced by
/// `encodeURIComponent` (e.g. `%5C` → `\`, `%3A` → `:`). Sufficient for local file paths.
fn percent_decode(s: &str) -> String {
    let mut result = String::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(h), Some(l)) = (hi, lo) {
                result.push(char::from((h * 16 + l) as u8));
                i += 3;
                continue;
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

/// Parse an `ara://` deep link URL into a frontend route path.
/// e.g. `ara://content/0xabc123` → `/content/0xabc123`
///
/// SECURITY: Only allow known route prefixes to prevent deep link injection.
fn parse_ara_deep_link(url: &str) -> Option<String> {
    let stripped = url.strip_prefix("ara://")?;
    if stripped.is_empty() || stripped.len() > 500 {
        return None;
    }
    let path = format!("/{}", stripped.trim_end_matches('/'));

    // Whitelist allowed route prefixes
    const ALLOWED_PREFIXES: &[&str] = &[
        "/content/",
        "/collection/",
        "/marketplace",
        "/library",
        "/publish",
        "/wallet",
        "/dashboard",
    ];
    if !ALLOWED_PREFIXES.iter().any(|p| path.starts_with(p) || path == p.trim_end_matches('/')) {
        tracing::warn!("Deep link path rejected (not whitelisted): {}", path);
        return None;
    }

    // Only allow safe characters (alphanumeric, slashes, hyphens, dots, underscores)
    if !path.chars().all(|c| c.is_ascii_alphanumeric() || "/-_.".contains(c)) {
        tracing::warn!("Deep link path rejected (invalid characters): {}", path);
        return None;
    }

    Some(path)
}

pub fn run() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,iroh=warn,iroh_net=warn"));

    // Write logs to platform-appropriate app data directory.
    // In release builds, this is the only way to see logs (no console window).
    let log_dir = {
        #[cfg(target_os = "windows")]
        {
            std::env::var("LOCALAPPDATA")
                .map(|p| std::path::PathBuf::from(p).join("one.ara.marketplace").join("logs"))
                .unwrap_or_else(|_| std::env::temp_dir().join("ara-marketplace-logs"))
        }
        #[cfg(target_os = "macos")]
        {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            std::path::PathBuf::from(home)
                .join("Library/Application Support/one.ara.marketplace/logs")
        }
        #[cfg(target_os = "linux")]
        {
            std::env::var("XDG_DATA_HOME")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| {
                    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
                    std::path::PathBuf::from(home).join(".local/share")
                })
                .join("one.ara.marketplace/logs")
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        {
            std::env::temp_dir().join("ara-marketplace-logs")
        }
    };
    let _ = std::fs::create_dir_all(&log_dir);

    let file_appender = tracing_appender::rolling::daily(&log_dir, "ara-marketplace.log");
    let (non_blocking_file, guard) = tracing_appender::non_blocking(file_appender);
    // Leak the guard so the background writer thread lives for the app's lifetime.
    std::mem::forget(guard);

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking_file)
        .with_ansi(false); // No ANSI escape codes in log files

    #[cfg(debug_assertions)]
    {
        // Debug builds: log to both stdout (terminal) and file
        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer())
            .with(file_layer)
            .init();
    }

    #[cfg(not(debug_assertions))]
    {
        // Release builds: log to file only (no console on Windows)
        tracing_subscriber::registry()
            .with(filter)
            .with(file_layer)
            .init();
    }

    tauri::Builder::default()
        // Serve preview-cache and download files via localasset:// (or https://localasset.localhost
        // on Windows). SECURITY: paths are sandboxed to the app data directory to prevent
        // arbitrary file reads via path traversal.
        .register_uri_scheme_protocol("localasset", |app, request| {
            use std::path::PathBuf;
            use tauri::http::Response;

            let forbidden = || {
                Response::builder()
                    .status(403)
                    .body(b"Forbidden: path outside app data directory".to_vec())
                    .unwrap()
            };

            // Resolve the app data directory (e.g. %LOCALAPPDATA%\one.ara.marketplace)
            let app_data_dir: PathBuf = match app.app_handle().path().app_local_data_dir() {
                Ok(d) => d,
                Err(_) => return forbidden(),
            };

            // Strip leading '/' and percent-decode the path component.
            let path_encoded = request.uri().path().trim_start_matches('/');
            let path = percent_decode(path_encoded);

            // SECURITY: Reject symlinks outright. A symlink inside app_data_dir could
            // point outside (e.g. to C:\Windows) and canonicalize would happily follow
            // it. The app never writes symlinks itself, so this is a safe blanket reject.
            if let Ok(meta) = std::fs::symlink_metadata(&path) {
                if meta.file_type().is_symlink() {
                    tracing::warn!("localasset:// rejected symlink: {}", path);
                    return forbidden();
                }
            }

            // SECURITY: Canonicalize the requested path and verify it is inside
            // the app data directory. This prevents path traversal attacks
            // (e.g. localasset://../../Windows/System32/config/SAM).
            let canonical = match std::fs::canonicalize(&path) {
                Ok(p) => p,
                Err(_) => {
                    return Response::builder()
                        .status(404)
                        .body(vec![])
                        .unwrap()
                }
            };
            let allowed_base = match std::fs::canonicalize(&app_data_dir) {
                Ok(p) => p,
                Err(_) => return forbidden(),
            };
            if !canonical.starts_with(&allowed_base) {
                tracing::warn!(
                    "localasset:// blocked path traversal attempt: {}",
                    path
                );
                return forbidden();
            }

            match std::fs::read(&canonical) {
                Ok(bytes) => {
                    // Detect MIME type from bytes first, fall back to extension.
                    let mime = infer::get(&bytes)
                        .map(|k| k.mime_type())
                        .unwrap_or_else(|| {
                            let p = path.to_lowercase();
                            if p.ends_with(".mp4") {
                                "video/mp4"
                            } else if p.ends_with(".webm") {
                                "video/webm"
                            } else if p.ends_with(".mov") {
                                "video/quicktime"
                            } else if p.ends_with(".png") {
                                "image/png"
                            } else if p.ends_with(".gif") {
                                "image/gif"
                            } else if p.ends_with(".jpg") || p.ends_with(".jpeg") {
                                "image/jpeg"
                            } else {
                                "application/octet-stream"
                            }
                        });
                    Response::builder()
                        .header("Content-Type", mime)
                        .body(bytes)
                        .unwrap()
                }
                Err(_) => Response::builder()
                    .status(404)
                    .body(vec![])
                    .unwrap(),
            }
        })
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // Focus the existing window when a second instance is launched
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
            // Check args for deep link URLs (second instance launched via ara://...)
            for arg in &args {
                if let Some(path) = parse_ara_deep_link(arg) {
                    let _ = app.emit("deep-link-navigate", path);
                }
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(setup::init)
        .invoke_handler(tauri::generate_handler![
            commands::wallet::connect_wallet,
            commands::wallet::disconnect_wallet,
            commands::wallet::get_balances,
            commands::content::publish_content,
            commands::content::confirm_publish,
            commands::content::get_content_detail,
            commands::content::search_content,
            commands::content::update_content,
            commands::content::confirm_update_content,
            commands::content::get_my_content,
            commands::content::get_published_content,
            commands::content::get_top_creators,
            commands::content::get_creator_content,
            commands::content::delist_content,
            commands::content::confirm_delist,
            commands::content::update_content_file,
            commands::content::confirm_content_file_update,
            commands::content::import_preview_assets,
            commands::content::get_preview_asset,
            // Arweave permanent storage
            commands::content::estimate_arweave_cost,
            commands::content::prepare_arweave_upload,
            commands::content::execute_arweave_upload,
            commands::content::confirm_arweave_upload,
            commands::content::get_arweave_config,
            commands::marketplace::purchase_content,
            commands::marketplace::confirm_purchase,
            commands::marketplace::get_library,
            commands::marketplace::open_downloaded_content,
            commands::marketplace::get_owned_content_path,
            commands::marketplace::has_purchased_content,
            commands::marketplace::redownload_content,
            commands::network::get_ara_price_usd,
            commands::marketplace::open_content_folder,
            commands::marketplace::broadcast_delivery_receipt,
            commands::marketplace::get_marketplace_address,
            commands::marketplace::tip_content,
            commands::marketplace::confirm_tip,
            commands::marketplace::get_receipt_count,
            commands::marketplace::list_for_resale,
            commands::marketplace::confirm_list_for_resale,
            commands::marketplace::cancel_resale_listing,
            commands::marketplace::confirm_cancel_listing,
            commands::marketplace::buy_resale,
            commands::marketplace::get_resale_listings,
            commands::marketplace::get_edition_info,
            commands::seeding::start_seeding,
            commands::seeding::stop_seeding,
            commands::seeding::get_seeder_stats,
            commands::staking::stake_ara,
            commands::staking::unstake_ara,
            commands::staking::stake_for_content,
            commands::staking::get_stake_info,
            commands::staking::claim_staking_reward,
            commands::staking::claim_token_staking_reward,
            commands::staking::prepare_claim_rewards,
            commands::staking::confirm_claim_rewards,
            commands::staking::get_reward_history,
            commands::staking::get_reward_pipeline,
            commands::staking::get_transaction_history,
            commands::staking::get_supported_tokens,
            commands::tx::wait_for_transaction,
            commands::sync::sync_content,
            commands::sync::sync_rewards,
            // Collections
            commands::collections::create_collection,
            commands::collections::confirm_create_collection,
            commands::collections::update_collection,
            commands::collections::confirm_update_collection,
            commands::collections::delete_collection,
            commands::collections::confirm_delete_collection,
            commands::collections::add_to_collection,
            commands::collections::confirm_add_to_collection,
            commands::collections::remove_from_collection,
            commands::collections::confirm_remove_from_collection,
            commands::collections::get_my_collections,
            commands::collections::get_collection,
            commands::collections::get_collection_items,
            commands::collections::get_all_collections,
            commands::collections::get_content_collection,
            commands::collections::get_top_collections,
            // Name registry
            commands::names::register_name,
            commands::names::confirm_register_name,
            commands::names::remove_display_name,
            commands::names::confirm_remove_name,
            commands::names::get_display_name,
            commands::names::get_display_names,
            commands::names::check_name_available,
            // Moderation
            commands::moderation::set_nsfw,
            commands::moderation::confirm_set_nsfw,
            commands::moderation::flag_content,
            commands::moderation::vote_on_flag,
            commands::moderation::resolve_flag,
            commands::moderation::appeal_flag,
            commands::moderation::get_moderation_status,
            // Analytics
            commands::analytics::get_price_history,
            commands::analytics::get_item_analytics,
            commands::analytics::get_top_collectors,
            commands::analytics::get_trending_content,
            commands::analytics::get_marketplace_overview,
            commands::analytics::get_collection_analytics,
            commands::analytics::get_collection_activity,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
