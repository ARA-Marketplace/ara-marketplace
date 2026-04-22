//! Integration tests that hit Sepolia RPC to verify on-chain state.
//! Run with: cargo test -p ara-chain --test sepolia_integration -- --ignored

use alloy::primitives::{address, Address, FixedBytes};
use ara_chain::{connect_http, ContractAddresses};

/// Sepolia contract addresses (from config.rs defaults — 2026-04-01 deployment)
fn test_addresses() -> ContractAddresses {
    ContractAddresses {
        ara_token: address!("A4c42cd49774d9B0af9C2D6BB88cf53b49b95b1b"),
        staking: address!("16e1CA6619FF0555BAFc43dEC9595C39776A2B63"),
        registry: address!("8C52B0b11cF5759312555ab1C6926e6Ce57297a0"),
        marketplace: address!("a133F5eb0aE369D627B13F0e283ACDC763Fb48c4"),
        collections: address!("606658d5935E788CccCDF9188308434130a7C671"),
        name_registry: address!("5C451d9B613468D4212AE31b5F139E759dD992FA"),
        moderation: Address::ZERO,
    }
}

const SEPOLIA_RPC: &str = "https://eth-sepolia.g.alchemy.com/v2/96LkbmgBuleyqzvpKIb15";
const TEST_WALLET: &str = "0x6EB8cE209AF20A979650d51d8B10975A42340A8a";

#[tokio::test]
#[ignore] // Requires Sepolia RPC — run with --ignored
async fn test_connect_and_read_block_number() {
    let chain = connect_http(SEPOLIA_RPC, test_addresses()).unwrap();
    let block = chain.get_block_number().await.unwrap();
    println!("Current Sepolia block: {block}");
    assert!(block > 10_000_000, "Block number should be > 10M on Sepolia");
}

#[tokio::test]
#[ignore]
async fn test_read_eth_balance() {
    let chain = connect_http(SEPOLIA_RPC, test_addresses()).unwrap();
    let wallet: Address = TEST_WALLET.parse().unwrap();
    let balance = chain.get_eth_balance(wallet).await.unwrap();
    println!("Test wallet ETH balance: {} wei", balance);
}

#[tokio::test]
#[ignore]
async fn test_read_ara_token_balance() {
    let chain = connect_http(SEPOLIA_RPC, test_addresses()).unwrap();
    let wallet: Address = TEST_WALLET.parse().unwrap();
    let balance = chain.token.balance_of(wallet).await.unwrap();
    println!("Test wallet ARA balance: {} wei", balance);
}

#[tokio::test]
#[ignore]
async fn test_read_staking_info() {
    let chain = connect_http(SEPOLIA_RPC, test_addresses()).unwrap();
    let wallet: Address = TEST_WALLET.parse().unwrap();
    let stake = chain.staking.staked_balance(wallet).await.unwrap();
    println!("Test wallet staked balance: {} wei", stake);

    // total_staked() may also revert if contract is in unexpected state
    match chain.staking.total_staked().await {
        Ok(total) => println!("Total staked: {} wei", total),
        Err(e) => println!("total_staked() reverted: {e}"),
    }

    // earned() may revert if user has never staked — that's expected behavior
    match chain.staking.earned(wallet).await {
        Ok(earned) => println!("Test wallet earned rewards: {} wei", earned),
        Err(e) => println!("earned() reverted (expected if never staked): {e}"),
    }
}

/// Regression test: both stats surfaced on the Dashboard must return real values
/// (we previously had them silently failing to zero).
#[tokio::test]
#[ignore]
async fn test_dashboard_chain_stats() {
    let chain = connect_http(SEPOLIA_RPC, test_addresses()).unwrap();
    let total_staked = chain.staking.total_staked().await
        .expect("total_staked must succeed");
    let total_rewards = chain.marketplace.total_rewards_claimed().await
        .expect("total_rewards_claimed must succeed");
    println!("Total staked: {} wei", total_staked);
    println!("Total rewards claimed: {} wei", total_rewards);
    assert!(total_staked > alloy::primitives::U256::ZERO, "total_staked should be > 0 with 4+ staker(s)");
}

#[tokio::test]
#[ignore]
async fn test_read_content_registry() {
    let chain = connect_http(SEPOLIA_RPC, test_addresses()).unwrap();
    // Read a zero content ID — should return default values, not error
    let zero_id: FixedBytes<32> = FixedBytes::ZERO;
    let is_active = chain.registry.is_active(zero_id).await.unwrap();
    assert!(!is_active, "Zero content ID should not be active");
    println!("Zero content is_active: {is_active}");

    // Read content count
    let count = chain.registry.get_content_count().await.unwrap();
    println!("Total content count on-chain: {count}");
}

#[tokio::test]
#[ignore]
async fn test_event_fetching() {
    let chain = connect_http(SEPOLIA_RPC, test_addresses()).unwrap();
    // Fetch a small window of events (Alchemy free tier limits to 10 blocks)
    let events = chain
        .events
        .fetch_events(10_342_496, Some(10_342_505))
        .await
        .unwrap();
    println!(
        "Events in block range 10342496-10342505: {} events",
        events.len()
    );
    for e in &events {
        println!("  Block {}: {:?}", e.block_number, e.event);
    }
}

#[tokio::test]
#[ignore]
async fn test_contract_addresses_match() {
    let chain = connect_http(SEPOLIA_RPC, test_addresses()).unwrap();
    let addrs = test_addresses();

    assert_eq!(chain.token_address(), addrs.ara_token);
    assert_eq!(chain.staking_address(), addrs.staking);
    assert_eq!(chain.registry_address(), addrs.registry);
    assert_eq!(chain.marketplace_address(), addrs.marketplace);
}
