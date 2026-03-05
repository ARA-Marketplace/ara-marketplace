use crate::commands::sync::{sync_content_impl, sync_rewards_impl};
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

    // Default supported ERC-20 payment tokens (Sepolia testnet)
    config.ethereum.supported_tokens = vec![
        ara_core::config::TokenConfig {
            address: "0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238".to_string(),
            symbol: "USDC".to_string(),
            decimals: 6,
        },
    ];

    // Use Irys devnet for testnets (mainnet Irys rejects Sepolia ETH)
    if config.ethereum.chain_id != 1 {
        config.arweave.node_url = "https://devnet.irys.xyz".to_string();
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

    // Detect contract redeployment: if stored addresses differ from config,
    // reset all sync state and clear stale data from old contracts.
    detect_contract_change(&db, &config);

    // Startup cleanup: remove stale unconfirmed publish attempts (active=0).
    // These are leftovers from publishes that were never signed in MetaMask.
    cleanup_stale_rows(db.conn());

    let app_handle = app.handle().clone();
    let state = AppState::new(config, db, app_handle.clone());
    app.manage(state);

    // Sync content from chain in the background, then start iroh + resume seeding
    let app_handle_sync = app_handle.clone();
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

    // Periodic background sync: poll the chain every 30s for new content events.
    // Emits "content-synced" so the Marketplace page auto-refreshes.
    let app_handle_rewards = app_handle_sync.clone();
    tauri::async_runtime::spawn(async move {
        // Wait for initial sync to finish first
        tokio::time::sleep(std::time::Duration::from_secs(35)).await;
        let state = app_handle_sync.state::<AppState>();
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            match sync_content_impl(&state).await {
                Ok(r) if r.new_content > 0 || r.delisted_content > 0 => {
                    info!(
                        "Periodic sync: {} new, {} delisted, block {}",
                        r.new_content, r.delisted_content, r.synced_to_block
                    );
                    let _ = app_handle_sync.emit("content-synced", ());
                }
                Err(e) => warn!("Periodic sync failed: {e}"),
                _ => {} // No new content, skip logging
            }
        }
    });

    // Periodic reward sync: poll chain every 30s for new purchases/distributions/claims.
    // Only runs when a wallet is connected. Emits "rewards-synced" so the Wallet page auto-refreshes.
    tauri::async_runtime::spawn(async move {
        // Wait for initial sync + content sync to settle
        tokio::time::sleep(std::time::Duration::from_secs(40)).await;
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            let state = app_handle_rewards.state::<AppState>();
            // Only sync if a wallet is connected
            let has_wallet = state.wallet_address.lock().await.is_some();
            if !has_wallet {
                continue;
            }
            match sync_rewards_impl(&state).await {
                Ok(r)
                    if r.distributions_found > 0
                        || r.claims_found > 0
                        || r.purchases_found > 0 =>
                {
                    info!(
                        "Periodic reward sync: {} dist, {} claims, {} purchases",
                        r.distributions_found, r.claims_found, r.purchases_found
                    );
                    let _ = app_handle_rewards.emit("rewards-synced", ());
                }
                Err(e) => warn!("Periodic reward sync failed: {e}"),
                _ => {} // No new events
            }
        }
    });

    // Handle deep link on first launch (app opened via ara:// URL).
    // The URL arrives as a command-line argument. Emit after a delay so the
    // frontend has time to mount its event listener.
    {
        let handle = app.handle().clone();
        for arg in std::env::args().skip(1) {
            if let Some(path) = crate::parse_ara_deep_link(&arg) {
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    let _ = handle.emit("deep-link-navigate", path);
                });
                break; // only handle the first deep link arg
            }
        }
    }

    info!("Ara Marketplace initialized successfully");
    Ok(())
}

/// Detect when contracts have been redeployed to new addresses.
/// Compares stored addresses in the DB config table against the current AppConfig.
/// If any address changed, wipe sync checkpoints and stale on-chain data so the
/// app re-indexes from the new deployment block.
fn detect_contract_change(db: &Database, config: &AppConfig) {
    let stored_marketplace = db.get_config("contract_marketplace");
    let current_marketplace = &config.ethereum.marketplace_address;

    let mut needs_reset = match &stored_marketplace {
        Some(addr) => !addr.eq_ignore_ascii_case(current_marketplace),
        None => {
            // No stored address: either truly first run (fresh DB) or upgrading
            // from an older build that didn't track contract addresses.
            // If last_synced_block exists, the DB has stale data from old contracts.
            let has_stale_sync = db.get_config("last_synced_block").is_some();
            if has_stale_sync {
                info!("Upgrading DB: no stored contract addresses but sync state exists — resetting");
                true
            } else {
                // Truly first run — just store addresses and deployment block
                let _ = db.set_config("contract_marketplace", current_marketplace);
                let _ = db.set_config("contract_registry", &config.ethereum.registry_address);
                let _ = db.set_config("contract_staking", &config.ethereum.staking_address);
                let _ = db.set_config("contract_token", &config.ethereum.ara_token_address);
                let _ = db.set_config("deployment_block", &config.ethereum.deployment_block.to_string());
                false
            }
        }
    };

    // Even if addresses match, detect stale sync state from an old deployment era.
    // This catches the case where addresses were stored by a previous run but
    // deployment_block has since advanced (contracts redeployed).
    if !needs_reset {
        let stale_sync = db
            .get_config("last_synced_block")
            .and_then(|s| s.parse::<u64>().ok())
            .map(|block| block < config.ethereum.deployment_block)
            .unwrap_or(false);

        let stale_deployment = db
            .get_config("deployment_block")
            .and_then(|s| s.parse::<u64>().ok())
            .map(|stored| stored != config.ethereum.deployment_block)
            .unwrap_or(false);

        if stale_sync {
            info!(
                "Stale sync detected: last_synced_block < deployment_block ({}). Resetting.",
                config.ethereum.deployment_block
            );
            needs_reset = true;
        } else if stale_deployment {
            info!(
                "Deployment block changed (new: {}). Resetting sync state.",
                config.ethereum.deployment_block
            );
            needs_reset = true;
        }
    }

    if needs_reset {
        info!(
            "Resetting sync state (old marketplace: {:?}, new: {}). Will re-index from block {}.",
            stored_marketplace, current_marketplace, config.ethereum.deployment_block
        );

        let conn = db.conn();
        // Clear sync checkpoints so we re-index from deployment_block
        let _ = conn.execute("DELETE FROM config WHERE key = 'last_synced_block'", []);
        let _ = conn.execute("DELETE FROM config WHERE key = 'rewards_sync_block'", []);
        // Clear stale on-chain data from old contracts
        let _ = conn.execute("DELETE FROM content", []);
        let _ = conn.execute("DELETE FROM purchases", []);
        let _ = conn.execute("DELETE FROM rewards", []);
        let _ = conn.execute("DELETE FROM delivery_receipts", []);
        let _ = conn.execute("DELETE FROM seeding", []);
        let _ = conn.execute("DELETE FROM content_seeders", []);

        // Store new addresses and deployment block
        let _ = db.set_config("contract_marketplace", current_marketplace);
        let _ = db.set_config("contract_registry", &config.ethereum.registry_address);
        let _ = db.set_config("contract_staking", &config.ethereum.staking_address);
        let _ = db.set_config("contract_token", &config.ethereum.ara_token_address);
        let _ = db.set_config("deployment_block", &config.ethereum.deployment_block.to_string());

        info!("Sync state reset complete — will re-index from block {}", config.ethereum.deployment_block);
    }
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
