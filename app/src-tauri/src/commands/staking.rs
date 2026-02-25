use crate::commands::types::{format_wei, hex_encode, parse_token_amount, TransactionRequest};
use crate::state::AppState;
use alloy::primitives::{Address, Bytes, FixedBytes, TxHash, U256, keccak256};
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

/// Build a `distributeRewards()` transaction for the content creator (fast path).
/// Reads locally collected delivery receipts from the DB, verifies each EIP-712
/// signature against on-chain buyers, and builds proportional weights.
/// Only the content creator (or global reporter) can sign this transaction.
#[tauri::command]
pub async fn prepare_distribute_rewards(
    state: State<'_, AppState>,
    content_id: String,
) -> Result<Vec<TransactionRequest>, String> {
    info!("Building distributeRewards tx for content: {}", content_id);

    let content_id_bytes: FixedBytes<32> = content_id
        .strip_prefix("0x")
        .unwrap_or(&content_id)
        .parse()
        .map_err(|e| format!("Invalid content ID: {e}"))?;

    let eth = &state.config.ethereum;
    let marketplace_addr = eth
        .marketplace_address
        .parse::<Address>()
        .map_err(|e| format!("Invalid marketplace address: {e}"))?;

    let domain_separator = compute_domain_separator(marketplace_addr, eth.chain_id);

    // Read receipts from DB
    let receipts = {
        let db = state.db.lock().await;
        db.get_receipts_for_content(&content_id)
            .map_err(|e| format!("DB read failed: {e}"))?
    };

    info!("Found {} raw receipts for {}", receipts.len(), content_id);

    // Verify each receipt's ECDSA signature and group by seeder ETH address
    let chain = state.chain_client()?;
    let mut seeder_receipt_counts: std::collections::HashMap<Address, u64> =
        std::collections::HashMap::new();

    for receipt in &receipts {
        let seeder_addr: Address = receipt
            .seeder_eth_address
            .parse()
            .unwrap_or(Address::ZERO);
        if seeder_addr == Address::ZERO {
            continue;
        }

        // Recover buyer from signature
        let Some(recovered_buyer) = verify_receipt_signature(
            &domain_separator,
            content_id_bytes,
            seeder_addr,
            receipt.timestamp as u64,
            &receipt.signature,
        ) else {
            warn!("Invalid receipt signature from alleged buyer {}", receipt.buyer_eth_address);
            continue;
        };

        // Confirm buyer address matches what's stored
        let stored_buyer: Address = receipt.buyer_eth_address.parse().unwrap_or(Address::ZERO);
        if recovered_buyer != stored_buyer {
            warn!("Signature mismatch: stored={}, recovered={}", stored_buyer, recovered_buyer);
            continue;
        }

        // Verify buyer purchased on-chain
        let purchased = chain
            .marketplace
            .has_purchased(content_id_bytes, recovered_buyer)
            .await
            .unwrap_or(false);
        if !purchased {
            warn!("Receipt from non-buyer {}", recovered_buyer);
            continue;
        }

        *seeder_receipt_counts.entry(seeder_addr).or_default() += 1;
    }

    if seeder_receipt_counts.is_empty() {
        return Err("No verified delivery receipts found — cannot distribute rewards".to_string());
    }

    // Get the caller's wallet address so we can auto-stake them if needed.
    let caller_addr: Option<Address> = {
        let wallet = state.wallet_address.lock().await;
        wallet.as_ref().and_then(|s| s.parse().ok())
    };

    // Fetch seeder minimum stake and staking contract address (needed for auto-stake tx).
    let staking_addr = eth
        .staking_address
        .parse::<Address>()
        .map_err(|e| format!("Invalid staking address: {e}"))?;

    let seeder_min_stake = chain
        .staking
        .seeder_min_stake()
        .await
        .unwrap_or(U256::from(1_000_000_000_000_000_000u128)); // default 1 ARA

    // Filter to eligible seeders and compute weights = receipt_count * content_stake.
    // If the caller (creator) has general stake but no content stake, we auto-stake them.
    let mut seeders: Vec<Address> = Vec::new();
    let mut weights: Vec<U256> = Vec::new();
    let mut needs_auto_stake = false;

    for (seeder, count) in &seeder_receipt_counts {
        let eligible = chain
            .staking
            .is_eligible_seeder(*seeder, content_id_bytes)
            .await
            .unwrap_or(false);

        if !eligible {
            // Check if this is the caller and they have enough general stake to auto-stake.
            if Some(*seeder) == caller_addr {
                let general_balance = chain
                    .staking
                    .staked_balance(*seeder)
                    .await
                    .unwrap_or(U256::ZERO);
                if general_balance >= seeder_min_stake {
                    info!(
                        "Creator {} has general stake {} but no content stake — will auto-stake",
                        seeder, general_balance
                    );
                    needs_auto_stake = true;
                    // Weight uses seeder_min_stake as the assumed post-stake amount.
                    let weight = U256::from(*count) * seeder_min_stake;
                    seeders.push(*seeder);
                    weights.push(weight);
                    continue;
                }
            }
            info!("Seeder {} is not eligible (insufficient content stake)", seeder);
            continue;
        }

        let stake = chain
            .staking
            .content_stake(*seeder, content_id_bytes)
            .await
            .unwrap_or(U256::ZERO);

        // Weight = receipt_count * stake (or just receipt_count if stake is 0)
        let weight = if stake > U256::ZERO {
            U256::from(*count) * stake
        } else {
            U256::from(*count)
        };

        seeders.push(*seeder);
        weights.push(weight);
        info!("Seeder {} → count={}, stake={}, weight={}", seeder, count, stake, weight);
    }

    if seeders.is_empty() {
        return Err(
            "No eligible seeders found — seeders must have staked ARA for this content. \
             As the publisher, stake at least 1 ARA for this content via the Staking page first."
                .to_string(),
        );
    }

    let distribute_calldata = MarketplaceClient::<()>::distribute_rewards_calldata(
        content_id_bytes,
        seeders,
        weights,
    );

    let mut txs: Vec<TransactionRequest> = Vec::new();

    // Prepend stakeForContent if the creator needs to be made eligible first.
    if needs_auto_stake {
        let stake_calldata =
            StakingClient::<()>::stake_for_content_calldata(content_id_bytes, seeder_min_stake);
        txs.push(TransactionRequest {
            to: format!("{staking_addr:#x}"),
            data: hex_encode(&stake_calldata),
            value: "0x0".to_string(),
            description: "Stake 1 ARA for your own content (seeder eligibility)".to_string(),
        });
    }

    txs.push(TransactionRequest {
        to: format!("{marketplace_addr:#x}"),
        data: hex_encode(&distribute_calldata),
        value: "0x0".to_string(),
        description: format!("Distribute rewards for content {}", &content_id[..10]),
    });

    Ok(txs)
}

/// Build a `publicDistributeWithProofs()` transaction for the trustless fallback.
/// Available after the distributionWindow has elapsed since the last purchase.
/// Any eligible seeder can call this — receipts are verified on-chain.
#[tauri::command]
pub async fn prepare_public_distribute(
    state: State<'_, AppState>,
    content_id: String,
) -> Result<Vec<TransactionRequest>, String> {
    info!("Building publicDistributeWithProofs tx for content: {}", content_id);

    let content_id_bytes: FixedBytes<32> = content_id
        .strip_prefix("0x")
        .unwrap_or(&content_id)
        .parse()
        .map_err(|e| format!("Invalid content ID: {e}"))?;

    let eth = &state.config.ethereum;
    let marketplace_addr = eth
        .marketplace_address
        .parse::<Address>()
        .map_err(|e| format!("Invalid marketplace address: {e}"))?;

    // Check if distribution window is open
    let chain = state.chain_client()?;
    let last_purchase = chain
        .marketplace
        .last_purchase_time(content_id_bytes)
        .await
        .unwrap_or(U256::ZERO);

    if last_purchase == U256::ZERO {
        return Err("No purchases have been made for this content".to_string());
    }

    let window = chain
        .marketplace
        .distribution_window()
        .await
        .unwrap_or(U256::from(30 * 24 * 3600u64));

    let now = U256::from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    );

    if now <= last_purchase + window {
        let opens_in = (last_purchase + window - now).saturating_to::<u64>();
        let hours = opens_in / 3600;
        let mins = (opens_in % 3600) / 60;
        return Err(format!(
            "Distribution window not yet open — opens in {}h {}m",
            hours, mins
        ));
    }

    // Read receipts from DB and group by seeder ETH address
    let receipts = {
        let db = state.db.lock().await;
        db.get_receipts_for_content(&content_id)
            .map_err(|e| format!("DB read failed: {e}"))?
    };

    // Group receipts by seeder (the contract verifies signatures; we just bundle them)
    let mut seeder_map: std::collections::HashMap<String, Vec<IMarketplace::SignedReceipt>> =
        std::collections::HashMap::new();

    for receipt in receipts {
        let sig_hex = receipt.signature.strip_prefix("0x").unwrap_or(&receipt.signature);
        let sig_bytes = alloy::hex::decode(sig_hex)
            .unwrap_or_default();
        if sig_bytes.len() != 65 {
            continue;
        }
        seeder_map
            .entry(receipt.seeder_eth_address)
            .or_default()
            .push(IMarketplace::SignedReceipt {
                timestamp: U256::from(receipt.timestamp as u64),
                signature: Bytes::from(sig_bytes),
            });
    }

    if seeder_map.is_empty() {
        return Err("No delivery receipts available to submit".to_string());
    }

    let mut bundles: Vec<IMarketplace::SeederBundle> = Vec::new();
    for (seeder_str, signed_receipts) in seeder_map {
        let seeder: Address = seeder_str.parse().unwrap_or(Address::ZERO);
        if seeder == Address::ZERO {
            continue;
        }
        bundles.push(IMarketplace::SeederBundle {
            seeder,
            receipts: signed_receipts,
        });
    }

    if bundles.is_empty() {
        return Err("No valid seeder bundles to submit".to_string());
    }

    let calldata = MarketplaceClient::<()>::public_distribute_calldata(content_id_bytes, bundles);

    Ok(vec![TransactionRequest {
        to: format!("{marketplace_addr:#x}"),
        data: hex_encode(&calldata),
        value: "0x0".to_string(),
        description: format!("Public distribute rewards for content {}", &content_id[..10]),
    }])
}

/// Compute the EIP-712 domain separator for the Marketplace contract.
fn compute_domain_separator(marketplace_addr: Address, chain_id: u64) -> [u8; 32] {
    let domain_type_hash = keccak256(
        b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)",
    );
    let name_hash = keccak256(b"AraMarketplace");
    let version_hash = keccak256(b"1");

    // ABI encode: (type_hash, name_hash, version_hash, chain_id, address)
    let mut data = Vec::with_capacity(160);
    data.extend_from_slice(domain_type_hash.as_slice());
    data.extend_from_slice(name_hash.as_slice());
    data.extend_from_slice(version_hash.as_slice());
    // chain_id as uint256 big-endian
    let chain_id_u256 = U256::from(chain_id);
    data.extend_from_slice(&chain_id_u256.to_be_bytes::<32>());
    // address left-padded to 32 bytes
    data.extend_from_slice(&[0u8; 12]);
    data.extend_from_slice(marketplace_addr.as_slice());

    *keccak256(&data)
}

/// Verify an EIP-712 DeliveryReceipt signature and return the recovered signer.
/// Returns None if the signature is invalid or recovery fails.
fn verify_receipt_signature(
    domain_separator: &[u8; 32],
    content_id: FixedBytes<32>,
    seeder_eth_address: Address,
    timestamp: u64,
    signature_hex: &str,
) -> Option<Address> {
    let receipt_type_hash = keccak256(
        b"DeliveryReceipt(bytes32 contentId,address seederEthAddress,uint256 timestamp)",
    );

    // ABI encode struct: (type_hash, content_id, seeder_address_padded, timestamp)
    let mut struct_data = Vec::with_capacity(128);
    struct_data.extend_from_slice(receipt_type_hash.as_slice());
    struct_data.extend_from_slice(content_id.as_slice());
    // address left-padded to 32 bytes
    struct_data.extend_from_slice(&[0u8; 12]);
    struct_data.extend_from_slice(seeder_eth_address.as_slice());
    // timestamp as uint256 big-endian
    struct_data.extend_from_slice(&U256::from(timestamp).to_be_bytes::<32>());

    let struct_hash = keccak256(&struct_data);

    // EIP-712: "\x19\x01" || domain_separator || struct_hash
    let mut digest_data = Vec::with_capacity(66);
    digest_data.extend_from_slice(b"\x19\x01");
    digest_data.extend_from_slice(domain_separator);
    digest_data.extend_from_slice(struct_hash.as_slice());

    let digest: FixedBytes<32> = keccak256(&digest_data);

    // Parse 65-byte signature
    let sig_hex_str = signature_hex.strip_prefix("0x").unwrap_or(signature_hex);
    let sig_bytes = alloy::hex::decode(sig_hex_str).ok()?;
    if sig_bytes.len() != 65 {
        return None;
    }

    // ECDSA recovery: sig = r(32) || s(32) || v(1)
    let sig = alloy::primitives::Signature::try_from(sig_bytes.as_slice()).ok()?;
    sig.recover_address_from_prehash(&digest).ok()
}

// ── Reward history and confirmation commands ──

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
    pub claimable_eth: String,
}

/// Called after distributeRewards tx is confirmed on-chain.
/// Parses the RewardsDistributed event from the receipt to record the user's share.
#[tauri::command]
pub async fn confirm_distribute_rewards(
    state: State<'_, AppState>,
    content_id: String,
    tx_hash: String,
) -> Result<(), String> {
    info!(
        "Confirming reward distribution: content={}, tx={}",
        content_id, tx_hash
    );

    let wallet = state.wallet_address.lock().await;
    let my_address_str = wallet.as_ref().ok_or("No wallet connected")?.clone();
    let my_address: Address = my_address_str
        .parse()
        .map_err(|e| format!("Invalid address: {e}"))?;
    drop(wallet);

    // Create a standalone HTTP provider (same pattern as tx.rs)
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

    // Decode RewardsDistributed event from logs to find our share
    let mut my_amount = U256::ZERO;
    for log in receipt.inner.logs() {
        if let Ok(event) = IMarketplace::RewardsDistributed::decode_log(&log.inner) {
            for (i, seeder) in event.seeders.iter().enumerate() {
                if *seeder == my_address {
                    if let Some(amount) = event.amounts.get(i) {
                        my_amount += *amount;
                    }
                }
            }
        }
    }

    let db = state.db.lock().await;
    if let Err(e) = db.insert_reward(&content_id, &my_amount.to_string(), &tx_hash, now) {
        warn!("Failed to record distribution: {}", e);
    }

    info!(
        "Recorded reward distribution: {} wei for {}",
        my_amount, content_id
    );
    Ok(())
}

/// Called after claimRewards tx is confirmed on-chain.
/// Parses the RewardClaimed event from the receipt and records in DB.
#[tauri::command]
pub async fn confirm_claim_rewards(
    state: State<'_, AppState>,
    tx_hash: String,
) -> Result<(), String> {
    info!("Confirming reward claim: tx={}", tx_hash);

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

    let mut claimed_amount = U256::ZERO;
    for log in receipt.inner.logs() {
        if let Ok(event) = IMarketplace::RewardClaimed::decode_log(&log.inner) {
            claimed_amount += event.amount;
        }
    }

    let db = state.db.lock().await;
    if let Err(e) = db.insert_reward_claim(&claimed_amount.to_string(), &tx_hash, now) {
        warn!("Failed to record claim: {}", e);
    }

    info!("Recorded reward claim: {} wei", claimed_amount);
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
    drop(db);

    // Query on-chain claimable rewards
    let claimable_wei = {
        let wallet = state.wallet_address.lock().await;
        if let Some(addr_str) = wallet.as_ref() {
            if let Ok(addr) = addr_str.parse::<Address>() {
                if let Ok(chain) = state.chain_client() {
                    chain
                        .marketplace
                        .claimable_rewards(addr)
                        .await
                        .unwrap_or(U256::ZERO)
                } else {
                    U256::ZERO
                }
            } else {
                U256::ZERO
            }
        } else {
            U256::ZERO
        }
    };

    // Lifetime earnings = claimed + currently claimable on-chain.
    // Also consider DB unclaimed distributions as a fallback (they may not yet
    // be reflected in the on-chain claimable mapping if distribute hasn't been called).
    let total_earned_from_chain = total_claimed_wei + claimable_wei;
    let total_earned_from_db = total_claimed_wei + total_unclaimed_wei;
    let total_earned = total_earned_from_chain.max(total_earned_from_db);

    Ok(RewardHistoryResponse {
        items,
        total_earned_eth: format_wei(total_earned),
        total_claimed_eth: format_wei(total_claimed_wei),
        claimable_eth: format_wei(claimable_wei),
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
