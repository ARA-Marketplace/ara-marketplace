mod blob_events;
mod commands;
mod gossip_actor;
mod setup;
mod state;

use tauri::Manager;
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

pub fn run() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,iroh=warn,iroh_net=warn"));

    // Write logs to %LOCALAPPDATA%\one.ara.marketplace\logs\ara-marketplace.log
    // In release builds, this is the only way to see logs (no console window).
    let log_dir = std::env::var("LOCALAPPDATA")
        .map(|p| {
            std::path::PathBuf::from(p)
                .join("one.ara.marketplace")
                .join("logs")
        })
        .unwrap_or_else(|_| std::env::temp_dir().join("ara-marketplace-logs"));
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
        // Serve preview-cache files via localasset:// (or https://localasset.localhost on Windows).
        // This avoids Tauri's built-in asset-protocol scope restrictions.
        .register_uri_scheme_protocol("localasset", |_app, request| {
            use tauri::http::Response;

            // Strip leading '/' and percent-decode the path component.
            let path_encoded = request.uri().path().trim_start_matches('/');
            let path = percent_decode(path_encoded);

            match std::fs::read(&path) {
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
                        .header("Access-Control-Allow-Origin", "*")
                        .body(bytes)
                        .unwrap()
                }
                Err(_) => Response::builder()
                    .status(404)
                    .body(vec![])
                    .unwrap(),
            }
        })
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // Focus the existing window when a second instance is launched
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
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
            commands::content::delist_content,
            commands::content::confirm_delist,
            commands::content::update_content_file,
            commands::content::confirm_content_file_update,
            commands::content::import_preview_assets,
            commands::content::get_preview_asset,
            commands::marketplace::purchase_content,
            commands::marketplace::confirm_purchase,
            commands::marketplace::get_library,
            commands::marketplace::open_downloaded_content,
            commands::marketplace::open_content_folder,
            commands::marketplace::broadcast_delivery_receipt,
            commands::marketplace::get_marketplace_address,
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
            commands::staking::prepare_claim_rewards,
            commands::staking::confirm_claim_rewards,
            commands::staking::get_reward_history,
            commands::staking::get_reward_pipeline,
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
