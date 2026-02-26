use crate::commands::types::{format_wei, hex_encode, parse_token_amount, TransactionRequest};
use crate::state::AppState;
use alloy::primitives::{Address, Bytes, FixedBytes, TxHash, U256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::sol_types::SolEvent;
use ara_chain::contracts::IMarketplace;
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

    Ok(StakeInfo {
        total_staked: format_wei(general_balance),
        general_balance: format_wei(general_balance),
        content_stakes: vec![],
    })
}

// ── Per-receipt reward claiming ──

/// Build a batch `claimDeliveryRewards` transaction.
/// Reads all delivery receipts from the DB where this wallet is the seeder,
/// checks on-chain which ones still have unclaimed buyer rewards, and builds
/// a single batch claim transaction.
#[tauri::command]
pub async fn prepare_claim_rewards(
    state: State<'_, AppState>,
) -> Result<Vec<TransactionRequest>, String> {
    info!("Building per-receipt claim rewards transaction");

    let wallet = state.wallet_address.lock().await;
    let wallet_str = wallet.as_ref().ok_or("No wallet connected")?.clone();
    drop(wallet);

    let eth = &state.config.ethereum;
    let marketplace_addr = eth
        .marketplace_address
        .parse::<Address>()
        .map_err(|e| format!("Invalid marketplace address: {e}"))?;

    // Read all delivery receipts where this wallet is the seeder
    let receipts = {
        let db = state.db.lock().await;
        db.get_receipts_for_seeder(&wallet_str)
            .map_err(|e| format!("DB read failed: {e}"))?
    };

    if receipts.is_empty() {
        return Err("No delivery receipts found — nothing to claim".to_string());
    }

    info!("Found {} receipts for seeder {}", receipts.len(), wallet_str);

    let chain = state.chain_client()?;

    // Filter to receipts that still have unclaimed buyer rewards on-chain
    let mut claims: Vec<IMarketplace::ClaimParams> = Vec::new();

    for receipt in &receipts {
        let content_id_bytes: FixedBytes<32> = receipt
            .content_id
            .strip_prefix("0x")
            .unwrap_or(&receipt.content_id)
            .parse()
            .unwrap_or_default();

        let buyer_addr: Address = receipt
            .buyer_eth_address
            .parse()
            .unwrap_or(Address::ZERO);
        if buyer_addr == Address::ZERO {
            continue;
        }

        // Check on-chain: is there still reward available for this buyer's purchase?
        let buyer_reward = chain
            .marketplace
            .get_buyer_reward(content_id_bytes, buyer_addr)
            .await
            .unwrap_or(U256::ZERO);

        if buyer_reward == U256::ZERO {
            // Already claimed or no reward available
            continue;
        }

        // Parse signature
        let sig_hex = receipt
            .signature
            .strip_prefix("0x")
            .unwrap_or(&receipt.signature);
        let sig_bytes = alloy::hex::decode(sig_hex).unwrap_or_default();
        if sig_bytes.len() != 65 {
            warn!(
                "Invalid signature length for receipt content={} buyer={}",
                receipt.content_id, receipt.buyer_eth_address
            );
            continue;
        }

        claims.push(IMarketplace::ClaimParams {
            contentId: content_id_bytes,
            buyer: buyer_addr,
            bytesServed: U256::from(receipt.bytes_served),
            timestamp: U256::from(receipt.timestamp as u64),
            signature: Bytes::from(sig_bytes),
        });
    }

    if claims.is_empty() {
        return Err("No unclaimed rewards found — all receipts have already been claimed".to_string());
    }

    info!("Building batch claim for {} receipts", claims.len());

    let calldata = MarketplaceClient::<()>::claim_delivery_rewards_calldata(claims);

    Ok(vec![TransactionRequest {
        to: format!("{marketplace_addr:#x}"),
        data: hex_encode(&calldata),
        value: "0x0".to_string(),
        description: "Collect all seeding rewards".to_string(),
    }])
}

// ── Reward pipeline (simplified for per-receipt model) ──

#[derive(Serialize, Deserialize)]
pub struct RewardPipelineResponse {
    /// Total ETH available to claim from unclaimed delivery receipts
    pub available_eth: String,
    /// Number of unclaimed delivery receipts
    pub receipt_count: u32,
    /// Lifetime total earnings (claimed + available)
    pub lifetime_earnings_eth: String,
}

/// Get reward pipeline data for the connected wallet.
/// Queries DB receipts and checks on-chain buyer rewards to determine what's claimable.
#[tauri::command]
pub async fn get_reward_pipeline(
    state: State<'_, AppState>,
) -> Result<RewardPipelineResponse, String> {
    let wallet = state.wallet_address.lock().await;
    let wallet_str = wallet.as_ref().ok_or("No wallet connected")?.clone();
    drop(wallet);

    // Read all delivery receipts where this wallet is the seeder
    let receipts = {
        let db = state.db.lock().await;
        db.get_receipts_for_seeder(&wallet_str)
            .map_err(|e| format!("DB read failed: {e}"))?
    };

    let chain = state.chain_client()?;

    let mut available = U256::ZERO;
    let mut claimable_count = 0u32;

    for receipt in &receipts {
        let content_id_bytes: FixedBytes<32> = receipt
            .content_id
            .strip_prefix("0x")
            .unwrap_or(&receipt.content_id)
            .parse()
            .unwrap_or_default();

        let buyer_addr: Address = receipt
            .buyer_eth_address
            .parse()
            .unwrap_or(Address::ZERO);
        if buyer_addr == Address::ZERO {
            continue;
        }

        // Check on-chain remaining reward for this buyer's purchase
        let buyer_reward = chain
            .marketplace
            .get_buyer_reward(content_id_bytes, buyer_addr)
            .await
            .unwrap_or(U256::ZERO);

        if buyer_reward > U256::ZERO {
            available += buyer_reward;
            claimable_count += 1;
        }
    }

    // Historical withdrawn from DB
    let withdrawn_str = {
        let db = state.db.lock().await;
        db.get_total_claimed_wei()
            .map_err(|e| format!("DB query failed: {e}"))?
    };
    let withdrawn: U256 = withdrawn_str.parse().unwrap_or(U256::ZERO);

    let lifetime = available + withdrawn;

    info!(
        "Reward pipeline for {}: available={}, receipts={}, lifetime={}",
        wallet_str,
        format_wei(available),
        claimable_count,
        format_wei(lifetime)
    );

    Ok(RewardPipelineResponse {
        available_eth: format_wei(available),
        receipt_count: claimable_count,
        lifetime_earnings_eth: format_wei(lifetime),
    })
}

// ── Reward history and confirmation ──

#[derive(Serialize, Deserialize)]
pub struct RewardHistoryItem {
    pub content_id: String,
    pub content_title: String,
    pub amount_eth: String,
    pub tx_hash: Option<String>,
    pub claimed: bool,
    pub distributed_at: u64,
}

#[derive(Serialize, Deserialize)]
pub struct RewardHistoryResponse {
    pub items: Vec<RewardHistoryItem>,
    pub total_earned_eth: String,
    pub total_claimed_eth: String,
}

/// Called after claimDeliveryRewards tx is confirmed on-chain.
/// Parses DeliveryRewardClaimed events from the receipt and records in DB.
#[tauri::command]
pub async fn confirm_claim_rewards(
    state: State<'_, AppState>,
    tx_hash: String,
) -> Result<(), String> {
    info!("Confirming delivery reward claim: tx={}", tx_hash);

    let rpc_url = &state.config.ethereum.rpc_url;
    let provider = ProviderBuilder::new()
        .connect_http(rpc_url.parse().map_err(|e| format!("Invalid RPC URL: {e}"))?);

    let hash: TxHash = tx_hash
        .parse()
        .map_err(|e| format!("Invalid tx hash: {e}"))?;

    let receipt = provider
        .get_transaction_receipt(hash)
        .await
        .map_err(|e| format!("Failed to get receipt: {e}"))?
        .ok_or("Receipt not found")?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let mut total_claimed = U256::ZERO;
    let db = state.db.lock().await;

    for log in receipt.inner.logs() {
        if let Ok(event) = IMarketplace::DeliveryRewardClaimed::decode_log(&log.inner) {
            let cid = format!("0x{}", alloy::hex::encode(event.contentId.as_slice()));
            total_claimed += event.amount;

            if let Err(e) = db.insert_reward(
                &cid,
                &event.amount.to_string(),
                &tx_hash,
                now,
            ) {
                warn!("Failed to record delivery reward claim for {}: {}", cid, e);
            }
        }
    }

    // Also check for the batch RewardsClaimed event
    for log in receipt.inner.logs() {
        if let Ok(event) = IMarketplace::RewardsClaimed::decode_log(&log.inner) {
            // Record the aggregate claim as well
            if let Err(e) = db.insert_reward_claim(
                &event.totalAmount.to_string(),
                &tx_hash,
                now,
            ) {
                warn!("Failed to record aggregate claim: {}", e);
            }
        }
    }

    info!("Recorded delivery reward claim: {} wei total", total_claimed);
    Ok(())
}

/// Get paginated reward history and summary totals.
#[tauri::command]
pub async fn get_reward_history(
    state: State<'_, AppState>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<RewardHistoryResponse, String> {
    let limit = limit.unwrap_or(20);
    let offset = offset.unwrap_or(0);

    let db = state.db.lock().await;
    let rows = db
        .get_reward_history(limit, offset)
        .map_err(|e| format!("DB query failed: {e}"))?;

    let items: Vec<RewardHistoryItem> = rows
        .into_iter()
        .map(|r| {
            let amount_wei: U256 = r.amount_wei.parse().unwrap_or(U256::ZERO);
            RewardHistoryItem {
                content_id: r.content_id.clone(),
                content_title: r.content_title.unwrap_or_else(|| {
                    if r.content_id == "claim" {
                        "Reward Claim".to_string()
                    } else {
                        "Unknown".to_string()
                    }
                }),
                amount_eth: format_wei(amount_wei),
                tx_hash: r.tx_hash,
                claimed: r.claimed,
                distributed_at: r.distributed_at as u64,
            }
        })
        .collect();

    let total_claimed_str = db
        .get_total_claimed_wei()
        .map_err(|e| format!("DB query failed: {e}"))?;
    let total_claimed_wei: U256 = total_claimed_str.parse().unwrap_or(U256::ZERO);

    let total_unclaimed_str = db
        .get_total_unclaimed_wei()
        .map_err(|e| format!("DB query failed: {e}"))?;
    let total_unclaimed_wei: U256 = total_unclaimed_str.parse().unwrap_or(U256::ZERO);

    let total_earned = total_claimed_wei + total_unclaimed_wei;

    Ok(RewardHistoryResponse {
        items,
        total_earned_eth: format_wei(total_earned),
        total_claimed_eth: format_wei(total_claimed_wei),
    })
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
