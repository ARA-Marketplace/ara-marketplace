mod commands;
mod gossip_actor;
mod setup;
mod state;

use tracing_subscriber::EnvFilter;

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,iroh=warn,iroh_net=warn")),
        )
        .init();

    tauri::Builder::default()
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
            commands::marketplace::purchase_content,
            commands::marketplace::confirm_purchase,
            commands::marketplace::get_library,
            commands::marketplace::open_downloaded_content,
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
