use crate::commands::types::{format_wei, hex_encode, parse_token_amount, TransactionRequest};
use crate::state::AppState;
use alloy::primitives::{Address, FixedBytes, U256};
use ara_chain::marketplace::MarketplaceClient;
use ara_chain::staking::StakingClient;
use ara_chain::token::TokenClient;
use serde::{Deserialize, Serialize};
use tauri::State;
use tracing::{info, warn};

#[derive(Serialize, Deserialize)]
pub struct StakeInfo {
    pub total_staked: String,
    pub general_balance: String,
    pub content_stakes: Vec<ContentStakeInfo>,
    pub claimable_rewards_eth: String,
}

#[derive(Serialize, Deserialize)]
pub struct ContentStakeInfo {
    pub content_id: String,
    pub title: String,
    pub amount_staked: String,
    pub is_eligible_seeder: bool,
}

/// Stake ARA tokens. Returns two transactions:
/// 1. Approve the staking contract to spend ARA
/// 2. Call stake(amount) on the staking contract
#[tauri::command]
pub async fn stake_ara(
    state: State<'_, AppState>,
    amount: String,
) -> Result<Vec<TransactionRequest>, String> {
    info!("Building stake transactions for {} ARA", amount);

    let amount_wei = parse_token_amount(&amount)?;
    let eth = &state.config.ethereum;

    let token_addr = eth
        .ara_token_address
        .parse::<Address>()
        .map_err(|e| format!("Invalid token address: {e}"))?;
    let staking_addr = eth
        .staking_address
        .parse::<Address>()
        .map_err(|e| format!("Invalid staking address: {e}"))?;

    // 1. approve(stakingContract, amount)
    let approve_data = TokenClient::<()>::approve_calldata(staking_addr, amount_wei);

    // 2. stake(amount)
    let stake_data = StakingClient::<()>::stake_calldata(amount_wei);

    Ok(vec![
        TransactionRequest {
            to: format!("{token_addr:#x}"),
            data: hex_encode(&approve_data),
            value: "0x0".to_string(),
            description: format!("Approve {} ARA for staking", amount),
        },
        TransactionRequest {
            to: format!("{staking_addr:#x}"),
            data: hex_encode(&stake_data),
            value: "0x0".to_string(),
            description: format!("Stake {} ARA", amount),
        },
    ])
}

/// Unstake ARA tokens. Returns one transaction.
#[tauri::command]
pub async fn unstake_ara(
    state: State<'_, AppState>,
    amount: String,
) -> Result<Vec<TransactionRequest>, String> {
    info!("Building unstake transaction for {} ARA", amount);

    let amount_wei = parse_token_amount(&amount)?;
    let staking_addr = state
        .config
        .ethereum
        .staking_address
        .parse::<Address>()
        .map_err(|e| format!("Invalid staking address: {e}"))?;

    let unstake_data = StakingClient::<()>::unstake_calldata(amount_wei);

    Ok(vec![TransactionRequest {
        to: format!("{staking_addr:#x}"),
        data: hex_encode(&unstake_data),
        value: "0x0".to_string(),
        description: format!("Unstake {} ARA", amount),
    }])
}

/// Stake ARA for a specific content item (required for seeding eligibility).
/// Returns approve + stakeForContent transactions.
#[tauri::command]
pub async fn stake_for_content(
    state: State<'_, AppState>,
    content_id: String,
    amount: String,
) -> Result<Vec<TransactionRequest>, String> {
    info!("Building content stake txs: {} ARA for {}", amount, content_id);

    let amount_wei = parse_token_amount(&amount)?;
    let eth = &state.config.ethereum;

    let token_addr = eth
        .ara_token_address
        .parse::<Address>()
        .map_err(|e| format!("Invalid token address: {e}"))?;
    let staking_addr = eth
        .staking_address
        .parse::<Address>()
        .map_err(|e| format!("Invalid staking address: {e}"))?;

    let content_id_bytes: FixedBytes<32> = content_id
        .strip_prefix("0x")
        .unwrap_or(&content_id)
        .parse()
        .map_err(|e| format!("Invalid content ID: {e}"))?;

    let approve_data = TokenClient::<()>::approve_calldata(staking_addr, amount_wei);
    let stake_data =
        StakingClient::<()>::stake_for_content_calldata(content_id_bytes, amount_wei);

    Ok(vec![
        TransactionRequest {
            to: format!("{token_addr:#x}"),
            data: hex_encode(&approve_data),
            value: "0x0".to_string(),
            description: format!("Approve {} ARA for content staking", amount),
        },
        TransactionRequest {
            to: format!("{staking_addr:#x}"),
            data: hex_encode(&stake_data),
            value: "0x0".to_string(),
            description: format!("Stake {} ARA for content", amount),
        },
    ])
}

/// Get staking information for the connected wallet.
#[tauri::command]
pub async fn get_stake_info(state: State<'_, AppState>) -> Result<StakeInfo, String> {
    let wallet = state.wallet_address.lock().await;
    let address_str = wallet.as_ref().ok_or("No wallet connected")?;
    let address: Address = address_str
        .parse()
        .map_err(|e| format!("Invalid address: {e}"))?;

    info!("Fetching stake info for {}", address_str);

    let chain = state.chain_client()?;

    let general_balance = chain
        .staking
        .staked_balance(address)
        .await
        .unwrap_or_else(|e| {
            warn!("Staking query failed: {e}");
            U256::ZERO
        });

    let claimable = chain
        .marketplace
        .claimable_rewards(address)
        .await
        .unwrap_or_else(|e| {
            warn!("Rewards query failed: {e}");
            U256::ZERO
        });

    Ok(StakeInfo {
        total_staked: format_wei(general_balance),
        general_balance: format_wei(general_balance),
        content_stakes: vec![], // Content stakes require indexing events (Phase 6)
        claimable_rewards_eth: format_wei(claimable),
    })
}

/// Build a claimRewards() transaction.
#[tauri::command]
pub async fn claim_rewards(
    state: State<'_, AppState>,
) -> Result<Vec<TransactionRequest>, String> {
    info!("Building claim rewards transaction");

    let marketplace_addr = state
        .config
        .ethereum
        .marketplace_address
        .parse::<Address>()
        .map_err(|e| format!("Invalid marketplace address: {e}"))?;

    let claim_data = MarketplaceClient::<()>::claim_rewards_calldata();

    Ok(vec![TransactionRequest {
        to: format!("{marketplace_addr:#x}"),
        data: hex_encode(&claim_data),
        value: "0x0".to_string(),
        description: "Claim ETH rewards".to_string(),
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_token_amount() {
        // Whole number
        assert_eq!(
            parse_token_amount("1").unwrap(),
            U256::from(1_000_000_000_000_000_000u128)
        );

        // Decimal
        assert_eq!(
            parse_token_amount("0.5").unwrap(),
            U256::from(500_000_000_000_000_000u128)
        );

        // Large amount
        assert_eq!(
            parse_token_amount("1000").unwrap(),
            U256::from(1000u64) * U256::from(1_000_000_000_000_000_000u128)
        );

        // Zero
        assert_eq!(parse_token_amount("0").unwrap(), U256::ZERO);

        // Invalid
        assert!(parse_token_amount("abc").is_err());
        assert!(parse_token_amount("1.2.3").is_err());
    }
}
