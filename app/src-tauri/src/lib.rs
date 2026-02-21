mod blob_events;
mod commands;
mod gossip_actor;
mod setup;
mod state;

use tauri::Manager;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

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
            commands::content::delist_content,
            commands::content::confirm_delist,
            commands::marketplace::purchase_content,
            commands::marketplace::confirm_purchase,
            commands::marketplace::get_library,
            commands::marketplace::open_downloaded_content,
            commands::marketplace::open_content_folder,
            commands::seeding::start_seeding,
            commands::seeding::stop_seeding,
            commands::seeding::get_seeder_stats,
            commands::staking::stake_ara,
            commands::staking::unstake_ara,
            commands::staking::stake_for_content,
            commands::staking::get_stake_info,
            commands::staking::claim_rewards,
            commands::tx::wait_for_transaction,
            commands::sync::sync_content,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
