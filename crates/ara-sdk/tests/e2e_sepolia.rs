//! End-to-end tests for the Ara SDK on Sepolia.
//! All operations go through the SDK public API.
//!
//! Required env vars: `SEPOLIA_RPC`, `DEPLOYER_KEY`, `TEST_WALLET_KEY`
//!
//! Run with:
//! ```bash
//! export SEPOLIA_RPC="https://eth-sepolia.g.alchemy.com/v2/YOUR_KEY"
//! export DEPLOYER_KEY="your_deployer_private_key_hex"
//! export TEST_WALLET_KEY="your_test_wallet_private_key_hex"
//! cargo test -p ara-sdk --test e2e_sepolia -- --ignored --nocapture --test-threads=1
//! ```

use alloy::primitives::{address, Address, FixedBytes, U256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::network::EthereumWallet;
use alloy::rpc::types::TransactionRequest as AlloyTxRequest;
use alloy::signers::local::PrivateKeySigner as AlloySigner;
use alloy::sol;
use alloy::sol_types::SolCall;
use ara_core::config::AppConfig;
use ara_sdk::{AraClient, PrivateKeySigner};

fn sepolia_rpc() -> String {
    std::env::var("SEPOLIA_RPC").expect("SEPOLIA_RPC env var required for E2E tests")
}

fn deployer_key() -> String {
    std::env::var("DEPLOYER_KEY").expect("DEPLOYER_KEY env var required for E2E tests")
}

fn test_wallet_key() -> String {
    std::env::var("TEST_WALLET_KEY").expect("TEST_WALLET_KEY env var required for E2E tests")
}

const ARA_TOKEN: Address = address!("53720EcdDF71fE618c7A5aEc99ac2e958ad4dF99");

// MockARAToken.mint is deployer-only, not in the SDK
sol! {
    #[sol(rpc)]
    interface IMockToken {
        function mint(address to, uint256 amount) external;
    }
}

fn test_config() -> AppConfig {
    AppConfig::default()
}

async fn test_client() -> AraClient {
    let key = test_wallet_key();
    let rpc = sepolia_rpc();
    let signer = PrivateKeySigner::new(
        &format!("0x{key}"),
        &rpc,
    );
    AraClient::builder()
        .config(test_config())
        .signer(signer)
        .build_in_memory()
        .await
        .unwrap()
}

/// Raw provider for deployer-only calls (mint)
fn deployer_provider() -> impl Provider + Clone {
    let key = deployer_key();
    let rpc = sepolia_rpc();
    let signer: AlloySigner = key.parse().expect("Invalid deployer key");
    let wallet = EthereumWallet::from(signer);
    ProviderBuilder::new()
        .wallet(wallet)
        .connect_http(rpc.parse().unwrap())
}

async fn mint_ara(to: Address, amount: U256) {
    let provider = deployer_provider();
    let calldata = IMockToken::mintCall { to, amount }.abi_encode();
    let tx = AlloyTxRequest::default()
        .to(ARA_TOKEN)
        .input(calldata.into());
    let pending = provider.send_transaction(tx).await.expect("Failed to mint");
    let receipt = pending.get_receipt().await.expect("Failed to get mint receipt");
    assert!(receipt.status(), "Mint reverted");
    println!("  Minted {} ARA to {to:#x}", ara_sdk::types::format_wei(amount));
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_e2e_01_balances() {
    println!("\n=== E2E 01: Check balances ===\n");

    let client = test_client().await;
    let addr = client.wallet_address().unwrap();
    println!("  Test wallet: {addr:#x}");

    let balances = client.get_balances(addr).await.unwrap();
    println!("  ETH:    {} ({})", balances.eth_display, balances.eth_wei);
    println!("  ARA:    {} ({})", balances.ara_display, balances.ara_wei);
    println!("  Staked: {} ({})", balances.staked_display, balances.staked_wei);

    // Should have some ETH for gas
    assert_ne!(balances.eth_wei, "0", "Test wallet needs ETH for gas");
}

#[tokio::test]
#[ignore]
async fn test_e2e_02_mint_ara() {
    println!("\n=== E2E 02: Mint ARA to test wallet ===\n");

    let client = test_client().await;
    let addr = client.wallet_address().unwrap();

    let before = client.get_balances(addr).await.unwrap();
    let before_ara: U256 = before.ara_wei.parse().unwrap();
    println!("  ARA before: {}", before.ara_display);

    // Mint 100 ARA
    let mint_amount = U256::from(100) * U256::from(10).pow(U256::from(18));
    mint_ara(addr, mint_amount).await;

    let after = client.get_balances(addr).await.unwrap();
    let after_ara: U256 = after.ara_wei.parse().unwrap();
    println!("  ARA after:  {}", after.ara_display);

    assert!(after_ara > before_ara, "ARA balance should increase after mint");
}

#[tokio::test]
#[ignore]
async fn test_e2e_03_stake() {
    println!("\n=== E2E 03: Stake ARA ===\n");

    let client = test_client().await;
    let addr = client.wallet_address().unwrap();

    let before = client.staking().get_stake_info(addr).await.unwrap();
    let before_stake: U256 = before.general_balance_wei.parse().unwrap();
    println!("  Staked before: {} wei", before.general_balance_wei);

    // Stake 10 ARA
    let txs = client.staking().prepare_stake("10").unwrap();
    assert_eq!(txs.len(), 2, "Stake should produce approve + stake");

    let hashes = client.execute_transactions(&txs).await.unwrap();
    println!("  Approve tx: {}", hashes[0]);
    println!("  Stake tx:   {}", hashes[1]);

    let after = client.staking().get_stake_info(addr).await.unwrap();
    let after_stake: U256 = after.general_balance_wei.parse().unwrap();
    println!("  Staked after:  {} wei", after.general_balance_wei);

    assert!(after_stake > before_stake, "Staked balance should increase");
}

#[tokio::test]
#[ignore]
async fn test_e2e_04_publish() {
    println!("\n=== E2E 04: Publish content ===\n");

    let client = test_client().await;

    let content_hash = FixedBytes::<32>::from([
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
        0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
        0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18,
        0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20,
    ]);

    let metadata = serde_json::json!({
        "title": "SDK E2E Test Content",
        "description": "Published via ara-sdk E2E test",
        "content_type": "application/octet-stream",
        "filename": "test.bin",
        "file_size": 1024,
    });

    let result = client
        .content()
        .prepare_publish(
            content_hash,
            metadata.to_string(),
            "0.001",
            1024,
            100,
            500,
            None,
        )
        .await
        .unwrap();

    println!("  Content hash: {}", result.content_hash);
    println!("  Transactions: {}", result.transactions.len());

    let hashes = client.execute_transactions(&result.transactions).await.unwrap();
    println!("  Publish tx: {}", hashes[0]);

    // We need the contentId from the receipt, but for the SDK flow
    // we'd extract it from the ContentPublished event.
    // For now, verify the tx went through.
    println!("  Content published successfully!");
}

#[tokio::test]
#[ignore]
async fn test_e2e_05_sync_content() {
    println!("\n=== E2E 05: Sync content from chain ===\n");

    let client = test_client().await;

    let result = client.sync().sync_content().await.unwrap();
    println!("  New content: {}", result.new_content);
    println!("  Updated:     {}", result.updated);
    println!("  Delisted:    {}", result.delisted);
    println!("  Block range: {} - {}", result.from_block, result.to_block);

    // Search for content
    let items = client.content().search("", 50).await.unwrap();
    println!("  Total content in DB: {}", items.len());
    assert!(!items.is_empty(), "Should have synced at least some content");
}

#[tokio::test]
#[ignore]
async fn test_e2e_06_purchase() {
    println!("\n=== E2E 06: Purchase content ===\n");

    let client = test_client().await;

    // First sync to get content in DB
    client.sync().sync_content().await.unwrap();

    // Find active content to purchase (not our own, not already purchased)
    let items = client.content().search("", 50).await.unwrap();
    let addr = client.wallet_address().unwrap();
    let addr_str = format!("{addr:#x}");

    // Skip content we created (can't buy own content) and free content
    // Try each eligible item until one purchase succeeds
    let candidates: Vec<_> = items.iter().filter(|i| {
        i.active
            && i.price_wei != "0"
            && !i.creator.eq_ignore_ascii_case(&addr_str)
    }).collect();

    println!("  Found {} purchasable candidates", candidates.len());

    let mut purchased = false;
    for item in &candidates {
        let cid_hex = item.content_id.strip_prefix("0x").unwrap_or(&item.content_id);
        let cid: FixedBytes<32> = cid_hex.parse().unwrap();
        let already = client.marketplace().has_purchased(cid, addr).await.unwrap();

        if already {
            println!("  Already purchased \"{}\" — skip", item.title);
            continue;
        }

        println!("  Trying: \"{}\" for {} {}", item.title, item.price_display, item.price_unit);

        let prep = client.marketplace().prepare_purchase(&item.content_id).await.unwrap();

        match client.execute_transactions(&prep.transactions).await {
            Ok(hashes) => {
                for (i, h) in hashes.iter().enumerate() {
                    println!("  Tx {}: {}", i, h);
                }
                client.marketplace().confirm_purchase(
                    &item.content_id,
                    &addr_str,
                    &item.price_wei,
                    &hashes.last().unwrap(),
                ).await.unwrap();
                println!("  Purchase confirmed!");
                purchased = true;
                break;
            }
            Err(e) => {
                println!("  Reverted: {e} — trying next item");
            }
        }
    }

    if !purchased {
        println!("  No content could be purchased (all reverted or already owned)");
    }
}

#[tokio::test]
#[ignore]
async fn test_e2e_07_staking_rewards() {
    println!("\n=== E2E 07: Check staking rewards ===\n");

    let client = test_client().await;
    let addr = client.wallet_address().unwrap();

    let info = client.staking().get_stake_info(addr).await.unwrap();
    println!("  Total staked (network): {} wei", info.total_staked_wei);
    println!("  User general stake:     {} wei", info.general_balance_wei);
    println!("  ETH rewards earned:     {} wei", info.eth_reward_earned_wei);

    let earned: U256 = info.eth_reward_earned_wei.parse().unwrap();
    if !earned.is_zero() {
        println!("  Has rewards to claim — preparing claim tx...");
        let txs = client.staking().prepare_claim_eth_reward(addr).await.unwrap();
        println!("  Claim tx prepared: {}", txs[0].description);
        // Don't actually claim in test to preserve state for other tests
    } else {
        println!("  No ETH rewards to claim yet");
    }
}

#[tokio::test]
#[ignore]
async fn test_e2e_08_collections() {
    println!("\n=== E2E 08: Collections ===\n");

    let client = test_client().await;
    let addr = client.wallet_address().unwrap();

    // Create collection
    let txs = client
        .collections()
        .prepare_create("SDK Test Collection", "Created via E2E test", "")
        .unwrap();
    let hashes = client.execute_transactions(&txs).await.unwrap();
    println!("  Create collection tx: {}", hashes[0]);

    // List creator's collections
    let collections = client.collections().get_creator_collections(addr).await.unwrap();
    println!("  Creator has {} collections", collections.len());
    assert!(!collections.is_empty(), "Should have at least 1 collection");

    let latest_id = collections.last().unwrap();
    let (owner, name, _desc, _, _, active) = client
        .collections()
        .get_collection(*latest_id)
        .await
        .unwrap();
    println!("  Latest collection: \"{}\" by {owner:#x} (active={active})", name);
    assert_eq!(owner, addr);
    assert_eq!(name, "SDK Test Collection");
}

#[tokio::test]
#[ignore]
async fn test_e2e_09_names() {
    println!("\n=== E2E 09: Name registry ===\n");

    let client = test_client().await;
    let addr = client.wallet_address().unwrap();

    // Generate unique name
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let name = format!("sdktest{}", ts % 100000);

    // Check availability
    let available = client.names().check_available(&name).await.unwrap();
    println!("  Name \"{name}\" available: {available}");

    if available {
        // Register
        let txs = client.names().prepare_register(&name).unwrap();
        let hashes = client.execute_transactions(&txs).await.unwrap();
        println!("  Register tx: {}", hashes[0]);

        // Verify on-chain
        let registered_name = client.names().get_name(addr).await.unwrap();
        println!("  On-chain name: \"{}\"", registered_name);
        assert_eq!(registered_name, name);

        // Confirm in local DB
        client.names().confirm_register(&format!("{addr:#x}"), &name).await.unwrap();

        // Remove
        let txs = client.names().prepare_remove().unwrap();
        let hashes = client.execute_transactions(&txs).await.unwrap();
        println!("  Remove tx: {}", hashes[0]);

        client.names().confirm_remove(&format!("{addr:#x}")).await.unwrap();
        println!("  Name removed");
    } else {
        println!("  Name not available, skipping register/remove");
    }
}

#[tokio::test]
#[ignore]
async fn test_e2e_10_edition_info() {
    println!("\n=== E2E 10: Edition info ===\n");

    let client = test_client().await;

    // Sync content first
    client.sync().sync_content().await.unwrap();

    let items = client.content().search("", 10).await.unwrap();
    if let Some(item) = items.first() {
        let content_id_hex = item.content_id.strip_prefix("0x").unwrap_or(&item.content_id);
        let content_id: FixedBytes<32> = content_id_hex.parse().unwrap();

        let info = client.content().get_edition_info(content_id).await.unwrap();
        println!("  Content: \"{}\"", item.title);
        println!("  Max supply:    {}", info.max_supply);
        println!("  Total minted:  {}", info.total_minted);
        println!("  Royalty bps:   {}", info.royalty_bps);
    } else {
        println!("  No content found for edition info check");
    }
}

#[tokio::test]
#[ignore]
async fn test_e2e_11_sync_rewards() {
    println!("\n=== E2E 11: Sync rewards ===\n");

    let client = test_client().await;

    let result = client.sync().sync_rewards().await.unwrap();
    println!("  Purchases found: {}", result.purchases_found);
    println!("  Claims found:    {}", result.claims_found);
    println!("  Listings found:  {}", result.listings_found);
    println!("  Synced to block: {}", result.synced_to_block);
}

#[tokio::test]
#[ignore]
async fn test_e2e_12_analytics() {
    println!("\n=== E2E 12: Analytics ===\n");

    let client = test_client().await;

    // Sync first to populate DB
    client.sync().sync_content().await.unwrap();
    client.sync().sync_rewards().await.unwrap();

    let overview = client.analytics().get_overview().await.unwrap();
    println!("  Total sales:       {}", overview.total_sales);
    println!("  Total volume:      {} ETH", overview.total_volume_eth);
    println!("  Total items:       {}", overview.total_items);
    println!("  Total collections: {}", overview.total_collections);

    let collectors = client.analytics().get_top_collectors(5).await.unwrap();
    println!("  Top collectors: {}", collectors.len());
    for c in &collectors {
        println!("    {} — {} purchases, {} ETH", c.address, c.purchase_count, c.total_spent_eth);
    }

    let trending = client.analytics().get_trending(5).await.unwrap();
    println!("  Trending items: {}", trending.len());
}

#[tokio::test]
#[ignore]
async fn test_e2e_13_unstake() {
    println!("\n=== E2E 13: Unstake ARA ===\n");

    let client = test_client().await;
    let addr = client.wallet_address().unwrap();

    let before = client.staking().get_stake_info(addr).await.unwrap();
    let before_stake: U256 = before.general_balance_wei.parse().unwrap();
    println!("  Staked before: {} wei", before.general_balance_wei);

    if before_stake.is_zero() {
        println!("  Nothing staked — skipping unstake");
        return;
    }

    // Unstake 1 ARA
    let txs = client.staking().prepare_unstake("1").unwrap();
    let hashes = client.execute_transactions(&txs).await.unwrap();
    println!("  Unstake tx: {}", hashes[0]);

    let after = client.staking().get_stake_info(addr).await.unwrap();
    let after_stake: U256 = after.general_balance_wei.parse().unwrap();
    println!("  Staked after:  {} wei", after.general_balance_wei);

    assert!(after_stake < before_stake, "Staked balance should decrease after unstake");
}
