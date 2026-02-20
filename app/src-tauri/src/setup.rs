use crate::commands::sync::sync_content_impl;
use crate::state::AppState;
use ara_core::config::AppConfig;
use ara_core::storage::Database;
use tauri::{Emitter, Manager};
use tracing::{info, warn};

pub fn init(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    info!("Initializing Ara Marketplace...");

    // Resolve data directory to the OS app data folder, NOT src-tauri/data.
    // Using a relative path inside src-tauri/ causes Tauri's hot-reload watcher
    // to rebuild the app every time iroh writes to its SQLite files.
    let app_data_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| format!("Failed to resolve app data dir: {e}"))?;
    std::fs::create_dir_all(&app_data_dir)
        .map_err(|e| format!("Failed to create app data dir: {e}"))?;

    let mut config = AppConfig::default();

    // Allow RPC URL override via environment variable (for Alchemy/Infura API keys)
    if let Ok(rpc_url) = std::env::var("SEPOLIA_RPC_URL").or_else(|_| std::env::var("ETH_RPC_URL")) {
        config.ethereum.rpc_url = rpc_url;
    }

    config.iroh.data_dir = app_data_dir
        .join("iroh")
        .to_string_lossy()
        .into_owned();
    config.storage.db_path = app_data_dir
        .join("ara-marketplace.db")
        .to_string_lossy()
        .into_owned();
    config.storage.downloads_dir = app_data_dir
        .join("downloads")
        .to_string_lossy()
        .into_owned();

    // Validate that we can parse the ARA token address (mainnet contract exists)
    if !config.ethereum.ara_token_address.is_empty() {
        config
            .ethereum
            .ara_token_address
            .parse::<alloy::primitives::Address>()
            .map_err(|e| format!("Invalid ARA token address: {e}"))?;
        info!(
            "ARA token address: {}",
            config.ethereum.ara_token_address
        );
    }

    info!("RPC endpoint: {}", config.ethereum.rpc_url);
    info!("Chain ID: {}", config.ethereum.chain_id);

    // Open local database
    let db = Database::open(&config.storage.db_path)
        .unwrap_or_else(|_| Database::open_in_memory().expect("Failed to create in-memory DB"));

    // Startup cleanup: remove stale unconfirmed publish attempts (active=0).
    // These are leftovers from publishes that were never signed in MetaMask.
    cleanup_stale_rows(db.conn());

    let app_handle = app.handle().clone();
    let state = AppState::new(config, db, app_handle.clone());
    app.manage(state);

    // Sync content from chain in the background, then start iroh + resume seeding
    tauri::async_runtime::spawn(async move {
        // Small delay to let the window render first
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        let state = app_handle.state::<AppState>();
        match sync_content_impl(&state).await {
            Ok(r) => {
                info!(
                    "Initial sync: {} new content, synced to block {}",
                    r.new_content, r.synced_to_block
                );
                // Notify frontend so the Marketplace page can auto-refresh
                let _ = app_handle.emit("content-synced", ());
            }
            Err(e) => warn!("Initial sync failed (will retry on manual refresh): {e}"),
        }

        // Eagerly start iroh so gossip resumes seeding announcements immediately.
        // This also makes the node discoverable to peers right away.
        if let Err(e) = state.ensure_iroh().await.map(drop) {
            warn!("Eager iroh start failed (will retry lazily): {e}");
        }
    });

    info!("Ara Marketplace initialized successfully");
    Ok(())
}

/// Remove stale unconfirmed rows (active=0) left over from publish attempts
/// that were never signed in MetaMask.
fn cleanup_stale_rows(conn: &rusqlite::Connection) {
    match conn.execute("DELETE FROM content WHERE active = 0", []) {
        Ok(n) if n > 0 => info!("Cleaned up {} stale unconfirmed content rows", n),
        Err(e) => warn!("Stale row cleanup failed: {e}"),
        _ => {}
    }
}
