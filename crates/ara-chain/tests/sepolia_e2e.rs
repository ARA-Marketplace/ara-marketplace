//! End-to-end integration tests on Sepolia that perform real transactions.
//! These tests use the deployer + test wallet private keys to:
//!   1. Mint ARA tokens (deployer → test wallet)
//!   2. Approve + Stake ARA
//!   3. Publish content
//!   4. Purchase content with ETH
//!   5. Verify reward state
//!
//! Run with: cargo test -p ara-chain --test sepolia_e2e -- --ignored --nocapture

use alloy::primitives::{address, Address, FixedBytes, U256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::network::EthereumWallet;
use alloy::rpc::types::TransactionRequest;
use alloy::signers::local::PrivateKeySigner;
use alloy::sol;
use alloy::sol_types::{SolCall, SolEvent};
use ara_chain::contracts::{IAraStaking, IAraContent, IMarketplace, IAraCollections};
use ara_chain::staking::StakingClient;
use ara_chain::token::TokenClient;
use ara_chain::content_token::ContentTokenClient;
use ara_chain::marketplace::MarketplaceClient;
use ara_chain::collections::CollectionsClient;

// Extend IAraToken with mint (only exists on MockARAToken)
sol! {
    #[sol(rpc)]
    interface IMockToken {
        function mint(address to, uint256 amount) external;
    }
}

// Contract addresses on Sepolia (2026-02-27 deployment with V2 staker rewards)
const ARA_TOKEN: Address = address!("53720EcdDF71fE618c7A5aEc99ac2e958ad4dF99");
const STAKING: Address = address!("fD41Ae37cD729b6a70e42641ea14187e213b29e6");
const REGISTRY: Address = address!("d45ff950bBC1c823F66C4EbdF72De23Eb02e4831");
const MARKETPLACE: Address = address!("D7992b6A863FBacE3BB58BFE5D31EAe580adF4E0");
const COLLECTIONS: Address = address!("59453f1f12D10e4B4210fae8188d666011292997");

const SEPOLIA_RPC: &str = "https://eth-sepolia.g.alchemy.com/v2/96LkbmgBuleyqzvpKIb15";
const DEPLOYER_KEY: &str = "e01acca40bdaa73a2ffc56715c14c7bb2f863ecd90872cd55d076f4bfc66d492";
const TEST_WALLET_KEY: &str = "693dc0b9fd1ef3fcdb61f1042e66578c433154bc04a71978a3d9684830a5eb74";

/// Helper: create a signing provider from a private key hex string.
fn make_provider(
    key_hex: &str,
) -> impl Provider + Clone {
    let signer: PrivateKeySigner = key_hex.parse().expect("Invalid private key");
    let wallet = EthereumWallet::from(signer);
    ProviderBuilder::new()
        .wallet(wallet)
        .connect_http(SEPOLIA_RPC.parse().unwrap())
}

/// Helper: create a read-only provider (no signer).
fn read_provider() -> impl Provider + Clone {
    ProviderBuilder::new().connect_http(SEPOLIA_RPC.parse().unwrap())
}

/// Helper: send a transaction and wait for receipt.
async fn send_tx(
    provider: &(impl Provider + Clone),
    to: Address,
    calldata: Vec<u8>,
    value: U256,
) -> alloy::rpc::types::TransactionReceipt {
    let tx = TransactionRequest::default()
        .to(to)
        .input(calldata.into())
        .value(value);

    let pending = provider
        .send_transaction(tx)
        .await
        .expect("Failed to send transaction");

    pending
        .get_receipt()
        .await
        .expect("Failed to get receipt")
}

/// Helper: decode a specific event from receipt logs.
fn find_event<E: SolEvent>(receipt: &alloy::rpc::types::TransactionReceipt) -> Option<E> {
    for log in receipt.inner.logs() {
        if let Ok(event) = E::decode_log(&log.inner) {
            return Some(event.data);
        }
    }
    None
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_e2e_step1_check_balances() {
    println!("\n=== Step 1: Check balances ===\n");

    let provider = read_provider();
    let deployer_signer: PrivateKeySigner = DEPLOYER_KEY.parse().unwrap();
    let deployer_addr = deployer_signer.address();
    let test_signer: PrivateKeySigner = TEST_WALLET_KEY.parse().unwrap();
    let test_addr = test_signer.address();

    println!("Deployer address: {deployer_addr}");
    println!("Test wallet address: {test_addr}");

    // ETH balances
    let deployer_eth = provider.get_balance(deployer_addr).await.unwrap();
    let test_eth = provider.get_balance(test_addr).await.unwrap();
    println!("Deployer ETH: {:.6} ETH", wei_to_eth(deployer_eth));
    println!("Test wallet ETH: {:.6} ETH", wei_to_eth(test_eth));

    // ARA balances
    let token = TokenClient::new(ARA_TOKEN, provider.clone());
    let deployer_ara = token.balance_of(deployer_addr).await.unwrap();
    let test_ara = token.balance_of(test_addr).await.unwrap();
    println!("Deployer ARA: {:.2} ARA", wei_to_eth(deployer_ara));
    println!("Test wallet ARA: {:.2} ARA", wei_to_eth(test_ara));

    // Staking state
    let staking = StakingClient::new(STAKING, provider.clone());
    let deployer_stake = staking.staked_balance(deployer_addr).await.unwrap();
    let test_stake = staking.staked_balance(test_addr).await.unwrap();
    println!("Deployer staked: {:.2} ARA", wei_to_eth(deployer_stake));
    println!("Test wallet staked: {:.2} ARA", wei_to_eth(test_stake));

    let deployer_eligible = staking.is_eligible_publisher(deployer_addr).await.unwrap();
    let test_eligible = staking.is_eligible_publisher(test_addr).await.unwrap();
    println!("Deployer eligible publisher: {deployer_eligible}");
    println!("Test wallet eligible publisher: {test_eligible}");

    // Content count
    let registry = ContentTokenClient::new(REGISTRY, provider.clone());
    let count = registry.get_content_count().await.unwrap();
    println!("Total content on-chain: {count}");

    assert!(deployer_eth > U256::ZERO, "Deployer needs ETH for gas");
}

#[tokio::test]
#[ignore]
async fn test_e2e_step2_mint_ara_to_test_wallet() {
    println!("\n=== Step 2: Mint ARA to test wallet ===\n");

    let deployer_provider = make_provider(DEPLOYER_KEY);
    let test_signer: PrivateKeySigner = TEST_WALLET_KEY.parse().unwrap();
    let test_addr = test_signer.address();

    // Check current ARA balance
    let reader = read_provider();
    let token = TokenClient::new(ARA_TOKEN, reader.clone());
    let before = token.balance_of(test_addr).await.unwrap();
    println!("Test wallet ARA before: {:.2} ARA", wei_to_eth(before));

    // Mint 100 ARA (100e18) to test wallet
    let mint_amount = U256::from(100) * U256::from(10).pow(U256::from(18));
    let calldata = IMockToken::mintCall {
        to: test_addr,
        amount: mint_amount,
    }
    .abi_encode();

    println!("Minting 100 ARA to {test_addr}...");
    let receipt = send_tx(&deployer_provider, ARA_TOKEN, calldata, U256::ZERO).await;
    println!("Mint tx: {:?} (status: {:?})", receipt.transaction_hash, receipt.status());
    assert!(receipt.status(), "Mint transaction should succeed");

    // Verify balance increased
    let after = token.balance_of(test_addr).await.unwrap();
    println!("Test wallet ARA after: {:.2} ARA", wei_to_eth(after));
    assert!(after >= before + mint_amount, "ARA balance should increase by mint amount");
}

#[tokio::test]
#[ignore]
async fn test_e2e_step3_stake_ara() {
    println!("\n=== Step 3: Approve & Stake ARA ===\n");

    let test_provider = make_provider(TEST_WALLET_KEY);
    let test_signer: PrivateKeySigner = TEST_WALLET_KEY.parse().unwrap();
    let test_addr = test_signer.address();

    let reader = read_provider();
    let staking = StakingClient::new(STAKING, reader.clone());
    let token = TokenClient::new(ARA_TOKEN, reader.clone());

    let before_stake = staking.staked_balance(test_addr).await.unwrap();
    let before_ara = token.balance_of(test_addr).await.unwrap();
    println!("Before - Staked: {:.2} ARA, Balance: {:.2} ARA", wei_to_eth(before_stake), wei_to_eth(before_ara));

    // Stake 20 ARA (need 10 min for publisher eligibility)
    let stake_amount = U256::from(20) * U256::from(10).pow(U256::from(18));

    if before_ara < stake_amount {
        println!("SKIP: Not enough ARA to stake (have {:.2}, need 20). Run step2 first.", wei_to_eth(before_ara));
        return;
    }

    // Step 3a: Approve staking contract to spend ARA
    let approve_calldata = TokenClient::<()>::approve_calldata(STAKING, stake_amount);
    println!("Approving {:.0} ARA for staking contract...", wei_to_eth(stake_amount));
    let receipt = send_tx(&test_provider, ARA_TOKEN, approve_calldata, U256::ZERO).await;
    println!("Approve tx: {:?} (status: {:?})", receipt.transaction_hash, receipt.status());
    assert!(receipt.status(), "Approve should succeed");

    // Step 3b: Stake
    let stake_calldata = StakingClient::<()>::stake_calldata(stake_amount);
    println!("Staking {:.0} ARA...", wei_to_eth(stake_amount));
    let receipt = send_tx(&test_provider, STAKING, stake_calldata, U256::ZERO).await;
    println!("Stake tx: {:?} (status: {:?})", receipt.transaction_hash, receipt.status());
    assert!(receipt.status(), "Stake should succeed");

    // Check Staked event
    if let Some(event) = find_event::<IAraStaking::Staked>(&receipt) {
        println!("Staked event: user={}, amount={:.2} ARA", event.user, wei_to_eth(event.amount));
    }

    // Verify
    let after_stake = staking.staked_balance(test_addr).await.unwrap();
    let is_eligible = staking.is_eligible_publisher(test_addr).await.unwrap();
    println!("After - Staked: {:.2} ARA, Eligible publisher: {is_eligible}", wei_to_eth(after_stake));
    assert!(after_stake >= before_stake + stake_amount, "Staked balance should increase");
    assert!(is_eligible, "Should be eligible publisher after staking 20 ARA");
}

#[tokio::test]
#[ignore]
async fn test_e2e_step4_publish_content() {
    println!("\n=== Step 4: Publish content ===\n");

    let test_provider = make_provider(TEST_WALLET_KEY);
    let test_signer: PrivateKeySigner = TEST_WALLET_KEY.parse().unwrap();
    let test_addr = test_signer.address();

    let reader = read_provider();
    let registry = ContentTokenClient::new(REGISTRY, reader.clone());
    let staking = StakingClient::new(STAKING, reader.clone());

    // Check eligibility
    let eligible = staking.is_eligible_publisher(test_addr).await.unwrap();
    if !eligible {
        println!("SKIP: Test wallet not eligible publisher. Run step3 first.");
        return;
    }

    let count_before = registry.get_content_count().await.unwrap();
    println!("Content count before: {count_before}");

    // Publish: unique content hash, 0.001 ETH price, unlimited edition
    let content_hash: FixedBytes<32> = FixedBytes::new(
        alloy::primitives::keccak256(
            format!("test-content-{}", std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
            ).as_bytes()
        ).0
    );
    let price_wei = U256::from(1_000_000_000_000_000u64); // 0.001 ETH
    let metadata_uri = r#"{"v":2,"title":"E2E Test Content","description":"Integration test","content_type":"other","file_size":1024}"#.to_string();

    let publish_calldata = ContentTokenClient::<()>::publish_content_calldata(
        content_hash,
        metadata_uri,
        price_wei,
        U256::from(1024),  // file_size
        U256::ZERO,        // maxSupply = 0 (unlimited)
        0u128,             // royaltyBps = 0
    );

    println!("Publishing content (hash: {content_hash})...");
    let receipt = send_tx(&test_provider, REGISTRY, publish_calldata, U256::ZERO).await;
    println!("Publish tx: {:?} (status: {:?})", receipt.transaction_hash, receipt.status());
    assert!(receipt.status(), "Publish should succeed");

    // Extract contentId from ContentPublished event
    let event = find_event::<IAraContent::ContentPublished>(&receipt)
        .expect("Should have ContentPublished event");
    let content_id = event.contentId;
    println!("ContentPublished event:");
    println!("  contentId: {content_id}");
    println!("  creator: {}", event.creator);
    println!("  price: {} wei ({:.6} ETH)", event.priceWei, wei_to_eth(event.priceWei));

    // Verify on-chain state
    let is_active = registry.is_active(content_id).await.unwrap();
    let creator = registry.get_creator(content_id).await.unwrap();
    let price = registry.get_price(content_id).await.unwrap();
    println!("On-chain: active={is_active}, creator={creator}, price={:.6} ETH", wei_to_eth(price));
    assert!(is_active);
    assert_eq!(creator, test_addr);
    assert_eq!(price, price_wei);

    let count_after = registry.get_content_count().await.unwrap();
    println!("Content count after: {count_after}");
    assert!(count_after > count_before, "Content count should increase");
}

#[tokio::test]
#[ignore]
async fn test_e2e_step5_publish_and_purchase() {
    println!("\n=== Step 5: Publish + Purchase (full flow) ===\n");

    let deployer_provider = make_provider(DEPLOYER_KEY);
    let deployer_signer: PrivateKeySigner = DEPLOYER_KEY.parse().unwrap();
    let deployer_addr = deployer_signer.address();
    let test_provider = make_provider(TEST_WALLET_KEY);
    let test_signer: PrivateKeySigner = TEST_WALLET_KEY.parse().unwrap();
    let test_addr = test_signer.address();

    let reader = read_provider();
    let _registry = ContentTokenClient::new(REGISTRY, reader.clone());
    let marketplace = MarketplaceClient::new(MARKETPLACE, reader.clone());
    let staking = StakingClient::new(STAKING, reader.clone());

    // Verify test wallet can publish
    let eligible = staking.is_eligible_publisher(test_addr).await.unwrap();
    if !eligible {
        println!("SKIP: Test wallet not eligible publisher. Run steps 2-3 first.");
        return;
    }

    // ── Publish from test wallet ──
    let content_hash: FixedBytes<32> = FixedBytes::new(
        alloy::primitives::keccak256(
            format!("purchase-test-{}", std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
            ).as_bytes()
        ).0
    );
    let price_wei = U256::from(1_000_000_000_000_000u64); // 0.001 ETH
    let metadata = r#"{"v":2,"title":"Purchase Test Item","description":"E2E purchase test","content_type":"music","file_size":2048}"#;
    let publish_calldata = ContentTokenClient::<()>::publish_content_calldata(
        content_hash, metadata.to_string(), price_wei,
        U256::from(2048), U256::ZERO, 0u128,
    );
    println!("Publishing content...");
    let receipt = send_tx(&test_provider, REGISTRY, publish_calldata, U256::ZERO).await;
    assert!(receipt.status(), "Publish should succeed");

    let publish_event = find_event::<IAraContent::ContentPublished>(&receipt)
        .expect("Should have ContentPublished event");
    let content_id = publish_event.contentId;
    println!("Published contentId: {content_id}");

    // ── Purchase from deployer ──
    let has_purchased_before = marketplace.has_purchased(content_id, deployer_addr).await.unwrap();
    assert!(!has_purchased_before, "Should not have purchased yet");

    let test_eth_before = reader.get_balance(test_addr).await.unwrap();

    let purchase_calldata = MarketplaceClient::<()>::purchase_calldata(content_id, price_wei);
    println!("Purchasing for 0.001 ETH from deployer wallet...");
    let receipt = send_tx(&deployer_provider, MARKETPLACE, purchase_calldata, price_wei).await;
    println!("Purchase tx: {:?} (status: {:?})", receipt.transaction_hash, receipt.status());
    assert!(receipt.status(), "Purchase should succeed");

    // Parse purchase event
    if let Some(event) = find_event::<IMarketplace::ContentPurchased>(&receipt) {
        println!("ContentPurchased:");
        println!("  buyer: {}", event.buyer);
        println!("  pricePaid: {:.6} ETH", wei_to_eth(event.pricePaid));
        println!("  creatorPayment: {:.6} ETH (85%)", wei_to_eth(event.creatorPayment));
        println!("  rewardAmount: {:.6} ETH (15%)", wei_to_eth(event.rewardAmount));
    }

    // ── Verify ──
    let has_purchased = marketplace.has_purchased(content_id, deployer_addr).await.unwrap();
    assert!(has_purchased, "Should have purchased");

    let test_eth_after = reader.get_balance(test_addr).await.unwrap();
    let creator_received = test_eth_after - test_eth_before;
    println!("Creator received: {:.6} ETH", wei_to_eth(creator_received));
    assert!(test_eth_after > test_eth_before, "Creator should receive ETH payment");

    // Check buyer reward pool (seeder claim pool)
    let buyer_reward = marketplace.get_buyer_reward(content_id, deployer_addr).await.unwrap();
    println!("Buyer reward pool (for seeders): {:.6} ETH", wei_to_eth(buyer_reward));

    println!("\nFull publish + purchase flow verified!");
}

// Additional sol! interfaces for V2 diagnostics and initialization
sol! {
    #[sol(rpc)]
    interface IMarketplaceV2 {
        function stakerRewardBps() external view returns (uint256);
        function resaleStakerRewardBps() external view returns (uint256);
        function resaleRewardBps() external view returns (uint256);
        function totalStakerRewardsForwarded() external view returns (uint256);
        function initializeV2(uint256 _stakerRewardBps, uint256 _resaleStakerRewardBps, uint256 _resaleRewardBps) external;
    }
    #[sol(rpc)]
    interface IStakingV2 {
        function authorizedMarketplace() external view returns (address);
        function rewardPerTokenStored() external view returns (uint256);
        function totalStakerRewardsDeposited() external view returns (uint256);
        function totalStakerRewardsClaimed() external view returns (uint256);
        function userRewardPerTokenPaid(address account) external view returns (uint256);
        function pendingRewards(address account) external view returns (uint256);
        function initializeV2(address _marketplace) external;
    }
}

/// Diagnostic: check V2 storage variables on deployed contracts.
/// If stakerRewardBps==0, call initializeV2 on both Marketplace and Staking.
#[tokio::test]
#[ignore]
async fn test_e2e_step5b_init_v2_if_needed() {
    println!("\n=== Step 5b: Diagnose & initialize V2 if needed ===\n");

    let reader = read_provider();

    // ── Read Marketplace V2 state ──
    let mp = IMarketplaceV2::new(MARKETPLACE, &reader);
    let staker_bps = mp.stakerRewardBps().call().await.unwrap();
    let resale_staker_bps = mp.resaleStakerRewardBps().call().await.unwrap();
    let resale_bps = mp.resaleRewardBps().call().await.unwrap();
    let fwd = mp.totalStakerRewardsForwarded().call().await.unwrap();
    println!("Marketplace V2 state:");
    println!("  stakerRewardBps: {staker_bps}");
    println!("  resaleStakerRewardBps: {resale_staker_bps}");
    println!("  resaleRewardBps: {resale_bps}");
    println!("  totalStakerRewardsForwarded: {fwd}");

    // ── Read Staking V2 state ──
    let sk = IStakingV2::new(STAKING, &reader);
    let auth_mp = sk.authorizedMarketplace().call().await.unwrap();
    let rpts = sk.rewardPerTokenStored().call().await.unwrap();
    let deposited = sk.totalStakerRewardsDeposited().call().await.unwrap();
    let claimed = sk.totalStakerRewardsClaimed().call().await.unwrap();
    println!("\nStaking V2 state:");
    println!("  authorizedMarketplace: {auth_mp}");
    println!("  rewardPerTokenStored: {rpts}");
    println!("  totalStakerRewardsDeposited: {deposited}");
    println!("  totalStakerRewardsClaimed: {claimed}");

    // Check test wallet specifics
    let test_signer: PrivateKeySigner = TEST_WALLET_KEY.parse().unwrap();
    let test_addr = test_signer.address();
    let user_rpt = sk.userRewardPerTokenPaid(test_addr).call().await.unwrap();
    let pending = sk.pendingRewards(test_addr).call().await.unwrap();
    println!("  userRewardPerTokenPaid[test]: {user_rpt}");
    println!("  pendingRewards[test]: {pending}");

    // ── Initialize V2 if needed ──
    if staker_bps == U256::ZERO {
        println!("\n*** stakerRewardBps is 0 — calling Marketplace.initializeV2(250, 100, 400) ***");
        let deployer = make_provider(DEPLOYER_KEY);
        let calldata = IMarketplaceV2::initializeV2Call {
            _stakerRewardBps: U256::from(250),
            _resaleStakerRewardBps: U256::from(100),
            _resaleRewardBps: U256::from(400),
        }.abi_encode();
        let receipt = send_tx(&deployer, MARKETPLACE, calldata, U256::ZERO).await;
        println!("Marketplace initializeV2 tx: {:?} (status: {:?})", receipt.transaction_hash, receipt.status());
        assert!(receipt.status(), "Marketplace initializeV2 should succeed");
    }

    if auth_mp == Address::ZERO {
        println!("\n*** authorizedMarketplace is ZERO — calling Staking.initializeV2(MARKETPLACE) ***");
        let deployer = make_provider(DEPLOYER_KEY);
        let calldata = IStakingV2::initializeV2Call {
            _marketplace: MARKETPLACE,
        }.abi_encode();
        let receipt = send_tx(&deployer, STAKING, calldata, U256::ZERO).await;
        println!("Staking initializeV2 tx: {:?} (status: {:?})", receipt.transaction_hash, receipt.status());
        assert!(receipt.status(), "Staking initializeV2 should succeed");
    }

    // Re-read after potential initialization
    if staker_bps == U256::ZERO || auth_mp == Address::ZERO {
        println!("\n=== Post-initialization state ===");
        let staker_bps2 = mp.stakerRewardBps().call().await.unwrap();
        let auth_mp2 = sk.authorizedMarketplace().call().await.unwrap();
        println!("  stakerRewardBps: {staker_bps2}");
        println!("  authorizedMarketplace: {auth_mp2}");
    }
}

#[tokio::test]
#[ignore]
async fn test_e2e_step6_staker_rewards() {
    println!("\n=== Step 6: Check staker rewards ===\n");

    let reader = read_provider();
    let staking = StakingClient::new(STAKING, reader.clone());
    let test_signer: PrivateKeySigner = TEST_WALLET_KEY.parse().unwrap();
    let test_addr = test_signer.address();

    let staked = staking.staked_balance(test_addr).await.unwrap();
    println!("Test wallet staked: {:.2} ARA", wei_to_eth(staked));

    if staked == U256::ZERO {
        println!("SKIP: Test wallet has no stake. Run steps 2-3 first.");
        return;
    }

    match staking.earned(test_addr).await {
        Ok(earned) => {
            println!("Earned staker rewards: {:.8} ETH ({} wei)", wei_to_eth(earned), earned);
            if earned > U256::ZERO {
                println!("Staker rewards accruing! Claiming...");
                let test_provider = make_provider(TEST_WALLET_KEY);
                let claim_calldata = StakingClient::<()>::claim_staking_reward_calldata();
                let receipt = send_tx(&test_provider, STAKING, claim_calldata, U256::ZERO).await;
                println!("Claim tx: {:?} (status: {:?})", receipt.transaction_hash, receipt.status());

                if let Some(event) = find_event::<IAraStaking::StakerRewardClaimed>(&receipt) {
                    println!("Claimed: {:.8} ETH to {}", wei_to_eth(event.amount), event.user);
                }
            } else {
                println!("No rewards yet — run step5b then purchase again to generate staker rewards.");
            }
        }
        Err(e) => println!("earned() reverted: {e}"),
    }
}

#[tokio::test]
#[ignore]
async fn test_e2e_step7_unstake() {
    println!("\n=== Step 7: Unstake ARA ===\n");

    let test_provider = make_provider(TEST_WALLET_KEY);
    let test_signer: PrivateKeySigner = TEST_WALLET_KEY.parse().unwrap();
    let test_addr = test_signer.address();

    let reader = read_provider();
    let staking = StakingClient::new(STAKING, reader.clone());
    let token = TokenClient::new(ARA_TOKEN, reader.clone());

    let staked_before = staking.staked_balance(test_addr).await.unwrap();
    let ara_before = token.balance_of(test_addr).await.unwrap();
    println!("Before - Staked: {:.2} ARA, Balance: {:.2} ARA", wei_to_eth(staked_before), wei_to_eth(ara_before));

    if staked_before == U256::ZERO {
        println!("SKIP: Nothing staked to unstake.");
        return;
    }

    // Unstake half
    let unstake_amount = staked_before / U256::from(2);
    let unstake_calldata = StakingClient::<()>::unstake_calldata(unstake_amount);
    println!("Unstaking {:.2} ARA...", wei_to_eth(unstake_amount));
    let receipt = send_tx(&test_provider, STAKING, unstake_calldata, U256::ZERO).await;
    println!("Unstake tx: {:?} (status: {:?})", receipt.transaction_hash, receipt.status());
    assert!(receipt.status(), "Unstake should succeed");

    if let Some(event) = find_event::<IAraStaking::Unstaked>(&receipt) {
        println!("Unstaked event: {:.2} ARA from {}", wei_to_eth(event.amount), event.user);
    }

    let staked_after = staking.staked_balance(test_addr).await.unwrap();
    let ara_after = token.balance_of(test_addr).await.unwrap();
    println!("After - Staked: {:.2} ARA, Balance: {:.2} ARA", wei_to_eth(staked_after), wei_to_eth(ara_after));
    assert!(staked_after < staked_before, "Staked balance should decrease");
    assert!(ara_after > ara_before, "ARA balance should increase (tokens returned)");
}

// ─── Collection Tests ────────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_e2e_step8_create_collection() {
    println!("\n=== Step 8: Create collection ===\n");

    let test_provider = make_provider(TEST_WALLET_KEY);
    let test_signer: PrivateKeySigner = TEST_WALLET_KEY.parse().unwrap();
    let test_addr = test_signer.address();
    let reader = read_provider();
    let collections = CollectionsClient::new(COLLECTIONS, reader.clone());

    let next_id_before = collections.next_collection_id().await.unwrap();
    println!("nextCollectionId before: {next_id_before}");

    let calldata = CollectionsClient::<()>::create_collection_calldata(
        "E2E Test Collection",
        "Integration test collection created by sepolia_e2e",
        "",
    );
    println!("Creating collection from test wallet ({test_addr})...");
    let receipt = send_tx(&test_provider, COLLECTIONS, calldata, U256::ZERO).await;
    println!("Tx: {:?} (status: {:?})", receipt.transaction_hash, receipt.status());
    assert!(receipt.status(), "Create collection should succeed");

    let event = find_event::<IAraCollections::CollectionCreated>(&receipt)
        .expect("Should have CollectionCreated event");
    println!(
        "CollectionCreated: id={}, creator={}, name={}",
        event.collectionId, event.creator, event.name
    );
    assert_eq!(event.creator, test_addr);
    assert_eq!(event.name, "E2E Test Collection");

    // Verify on-chain state
    let (creator, name, desc, _banner, _created_at, active) =
        collections.get_collection(event.collectionId).await.unwrap();
    println!("On-chain: creator={creator}, name={name}, desc={desc}, active={active}");
    assert_eq!(creator, test_addr);
    assert_eq!(name, "E2E Test Collection");
    assert!(active);

    let next_id_after = collections.next_collection_id().await.unwrap();
    assert!(next_id_after > next_id_before, "nextCollectionId should increase");
    println!("nextCollectionId after: {next_id_after}");
}

#[tokio::test]
#[ignore]
async fn test_e2e_step9_publish_into_collection() {
    println!("\n=== Step 9: Publish content and add to collection ===\n");

    let test_provider = make_provider(TEST_WALLET_KEY);
    let test_signer: PrivateKeySigner = TEST_WALLET_KEY.parse().unwrap();
    let test_addr = test_signer.address();
    let reader = read_provider();
    let collections = CollectionsClient::new(COLLECTIONS, reader.clone());
    let staking = StakingClient::new(STAKING, reader.clone());

    // Check publisher eligibility
    let eligible = staking.is_eligible_publisher(test_addr).await.unwrap();
    if !eligible {
        println!("SKIP: Test wallet not an eligible publisher. Run step 3 first.");
        return;
    }

    // Find the latest collection
    let next_id = collections.next_collection_id().await.unwrap();
    if next_id == U256::ZERO {
        println!("SKIP: No collections exist. Run step 8 first.");
        return;
    }
    let collection_id = next_id - U256::from(1);
    let (creator, name, _, _, _, active) =
        collections.get_collection(collection_id).await.unwrap();
    println!("Using collection {collection_id}: name={name}, creator={creator}, active={active}");
    assert_eq!(creator, test_addr, "Collection creator should be test wallet");
    assert!(active, "Collection should be active");

    // Publish a new content item
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let content_hash: FixedBytes<32> = FixedBytes::new(
        alloy::primitives::keccak256(format!("collection-test-{ts}").as_bytes()).0,
    );
    let price_wei = U256::from(1_000_000_000_000_000u64); // 0.001 ETH
    let metadata = format!(
        r#"{{"v":2,"title":"Collection Test Item {ts}","description":"E2E test item for collection","content_type":"document","file_size":1024}}"#
    );

    let publish_calldata = ContentTokenClient::<()>::publish_content_calldata(
        content_hash,
        metadata,
        price_wei,
        U256::from(1024),
        U256::ZERO,
        0u128,
    );
    println!("Publishing content (hash={content_hash})...");
    let receipt = send_tx(&test_provider, REGISTRY, publish_calldata, U256::ZERO).await;
    assert!(receipt.status(), "Publish should succeed");

    let pub_event = find_event::<IAraContent::ContentPublished>(&receipt)
        .expect("Should have ContentPublished event");
    let content_id = pub_event.contentId;
    println!("ContentPublished: contentId={content_id}");

    // Add to collection
    let add_calldata = CollectionsClient::<()>::add_item_calldata(collection_id, content_id);
    println!("Adding contentId={content_id} to collection {collection_id}...");
    let receipt = send_tx(&test_provider, COLLECTIONS, add_calldata, U256::ZERO).await;
    println!("Tx: {:?} (status: {:?})", receipt.transaction_hash, receipt.status());
    assert!(receipt.status(), "Add item should succeed");

    let add_event = find_event::<IAraCollections::ItemAddedToCollection>(&receipt)
        .expect("Should have ItemAddedToCollection event");
    println!(
        "ItemAddedToCollection: collectionId={}, contentId={}",
        add_event.collectionId, add_event.contentId
    );
    assert_eq!(add_event.collectionId, collection_id);
    assert_eq!(add_event.contentId, content_id);

    // Verify contentCollection mapping
    let mapped_collection = collections.content_collection(content_id).await.unwrap();
    println!("contentCollection({content_id}) = {mapped_collection}");
    assert_eq!(mapped_collection, collection_id, "Content should be mapped to collection");
}

#[tokio::test]
#[ignore]
async fn test_e2e_step10_verify_collection_state() {
    println!("\n=== Step 10: Verify collection state (read-only) ===\n");

    let reader = read_provider();
    let collections = CollectionsClient::new(COLLECTIONS, reader.clone());
    let test_signer: PrivateKeySigner = TEST_WALLET_KEY.parse().unwrap();
    let test_addr = test_signer.address();

    // Find the latest collection
    let next_id = collections.next_collection_id().await.unwrap();
    if next_id == U256::ZERO {
        println!("SKIP: No collections exist.");
        return;
    }
    let collection_id = next_id - U256::from(1);

    let (creator, name, desc, banner, created_at, active) =
        collections.get_collection(collection_id).await.unwrap();
    println!("Collection {collection_id}:");
    println!("  creator:    {creator}");
    println!("  name:       {name}");
    println!("  description:{desc}");
    println!("  banner:     {banner}");
    println!("  createdAt:  {created_at}");
    println!("  active:     {active}");
    assert_eq!(creator, test_addr, "Creator should be test wallet");
    assert!(active, "Collection should be active");

    let items = collections.get_collection_items(collection_id).await.unwrap();
    println!("  items ({}):", items.len());
    for (i, item) in items.iter().enumerate() {
        println!("    [{i}] {item}");
        // Verify reverse mapping
        let mapped = collections.content_collection(*item).await.unwrap();
        assert_eq!(mapped, collection_id, "Item should map back to this collection");
    }
    assert!(!items.is_empty(), "Collection should have at least 1 item (from step 9)");

    // Verify creator collections list
    let creator_colls = collections.get_creator_collections(test_addr).await.unwrap();
    println!("Creator collections: {:?}", creator_colls);
    assert!(
        creator_colls.contains(&collection_id),
        "Creator's collection list should include this collection"
    );
}

#[tokio::test]
#[ignore]
async fn test_e2e_step11_remove_from_collection() {
    println!("\n=== Step 11: Remove item from collection ===\n");

    let test_provider = make_provider(TEST_WALLET_KEY);
    let reader = read_provider();
    let collections = CollectionsClient::new(COLLECTIONS, reader.clone());

    // Find latest collection and its items
    let next_id = collections.next_collection_id().await.unwrap();
    if next_id == U256::ZERO {
        println!("SKIP: No collections exist.");
        return;
    }
    let collection_id = next_id - U256::from(1);
    let items_before = collections.get_collection_items(collection_id).await.unwrap();
    if items_before.is_empty() {
        println!("SKIP: Collection has no items. Run step 9 first.");
        return;
    }

    let content_id = items_before[0];
    println!(
        "Removing contentId={content_id} from collection {collection_id} ({} items before)...",
        items_before.len()
    );

    let remove_calldata = CollectionsClient::<()>::remove_item_calldata(collection_id, content_id);
    let receipt = send_tx(&test_provider, COLLECTIONS, remove_calldata, U256::ZERO).await;
    println!("Tx: {:?} (status: {:?})", receipt.transaction_hash, receipt.status());
    assert!(receipt.status(), "Remove item should succeed");

    let rm_event = find_event::<IAraCollections::ItemRemovedFromCollection>(&receipt)
        .expect("Should have ItemRemovedFromCollection event");
    println!(
        "ItemRemovedFromCollection: collectionId={}, contentId={}",
        rm_event.collectionId, rm_event.contentId
    );
    assert_eq!(rm_event.collectionId, collection_id);
    assert_eq!(rm_event.contentId, content_id);

    // Verify contentCollection returns 0 (unlinked)
    let mapped = collections.content_collection(content_id).await.unwrap();
    println!("contentCollection({content_id}) = {mapped} (expected 0)");
    assert_eq!(mapped, U256::ZERO, "Content should no longer be in any collection");

    let items_after = collections.get_collection_items(collection_id).await.unwrap();
    println!("Items after removal: {}", items_after.len());
    assert_eq!(
        items_after.len(),
        items_before.len() - 1,
        "Item count should decrease by 1"
    );
    assert!(
        !items_after.contains(&content_id),
        "Removed item should not be in items list"
    );
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn wei_to_eth(wei: U256) -> f64 {
    let s = wei.to_string();
    let n: f64 = s.parse().unwrap_or(0.0);
    n / 1e18
}
