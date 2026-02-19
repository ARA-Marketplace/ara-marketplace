use crate::commands::types::format_wei;
use crate::state::AppState;
use alloy::primitives::{Address, U256};
use serde::{Deserialize, Serialize};
use tauri::State;
use tracing::{info, warn};

#[derive(Serialize, Deserialize)]
pub struct Balances {
    pub eth_balance: String,
    pub ara_balance: String,
    pub ara_staked: String,
    pub claimable_rewards: String,
}

#[tauri::command]
pub async fn connect_wallet(
    state: State<'_, AppState>,
    address: String,
) -> Result<String, String> {
    info!("Connecting wallet: {}", address);

    // Validate that it's a valid Ethereum address
    address
        .parse::<Address>()
        .map_err(|e| format!("Invalid address: {e}"))?;

    let mut wallet = state.wallet_address.lock().await;
    *wallet = Some(address.clone());
    Ok(address)
}

#[tauri::command]
pub async fn disconnect_wallet(state: State<'_, AppState>) -> Result<(), String> {
    info!("Disconnecting wallet");
    let mut wallet = state.wallet_address.lock().await;
    *wallet = None;
    Ok(())
}

#[tauri::command]
pub async fn get_balances(state: State<'_, AppState>) -> Result<Balances, String> {
    let wallet = state.wallet_address.lock().await;
    let address_str = wallet.as_ref().ok_or("No wallet connected")?;
    let address: Address = address_str
        .parse()
        .map_err(|e| format!("Invalid address: {e}"))?;

    let chain = state.chain_client()?;

    // Query ETH balance
    let eth_balance = chain
        .get_eth_balance(address)
        .await
        .map_err(|e| format!("Failed to get ETH balance: {e}"))?;

    // Query ARA token balance
    let ara_balance = chain
        .token
        .balance_of(address)
        .await
        .map_err(|e| format!("Failed to get ARA balance: {e}"))?;

    // Query staked ARA (may fail if staking contract not deployed)
    let ara_staked = chain
        .staking
        .staked_balance(address)
        .await
        .unwrap_or_else(|e| {
            warn!("Staking query failed (contract may not be deployed): {e}");
            U256::ZERO
        });

    // Query claimable rewards (may fail if marketplace not deployed)
    let claimable_rewards = chain
        .marketplace
        .claimable_rewards(address)
        .await
        .unwrap_or_else(|e| {
            warn!("Rewards query failed (contract may not be deployed): {e}");
            U256::ZERO
        });

    info!(
        "Balances for {}: ETH={}, ARA={}, staked={}, rewards={}",
        address_str, eth_balance, ara_balance, ara_staked, claimable_rewards
    );

    Ok(Balances {
        eth_balance: format_wei(eth_balance),
        ara_balance: format_wei(ara_balance),
        ara_staked: format_wei(ara_staked),
        claimable_rewards: format_wei(claimable_rewards),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_wei() {
        // 0 wei
        assert_eq!(format_wei(U256::ZERO), "0.0");
        // 1 ETH = 10^18 wei
        assert_eq!(
            format_wei(U256::from(1_000_000_000_000_000_000u128)),
            "1.0"
        );
        // 0.1 ETH
        assert_eq!(
            format_wei(U256::from(100_000_000_000_000_000u128)),
            "0.1"
        );
        // 1.5 ETH
        assert_eq!(
            format_wei(U256::from(1_500_000_000_000_000_000u128)),
            "1.5"
        );
        // 0.000000000000000001 ETH (1 wei)
        assert_eq!(format_wei(U256::from(1u64)), "0.000000000000000001");
        // Large amount: 1000 ETH
        assert_eq!(
            format_wei(U256::from(1000u64) * U256::from(1_000_000_000_000_000_000u128)),
            "1000.0"
        );
    }
}
