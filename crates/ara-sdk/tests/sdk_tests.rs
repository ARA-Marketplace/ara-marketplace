//! SDK unit tests: builder, types, calldata generation.

use alloy::primitives::{address, Address, FixedBytes, U256};
use ara_core::config::AppConfig;
use ara_sdk::types::{format_token_amount, format_wei, hex_encode, parse_amount};
use ara_sdk::{AraClient, PrivateKeySigner, Signer};

// ─── Builder tests ──────────────────────────────────────────────────────────

#[tokio::test]
async fn test_builder_default_config() {
    let client = AraClient::builder().build_in_memory().await.unwrap();
    assert_eq!(client.config().ethereum.chain_id, 11155111);
    assert!(client.wallet_address().is_none());
}

#[tokio::test]
async fn test_builder_custom_config() {
    let mut config = AppConfig::default();
    config.ethereum.chain_id = 1;
    let client = AraClient::builder()
        .config(config)
        .build_in_memory()
        .await
        .unwrap();
    assert_eq!(client.config().ethereum.chain_id, 1);
}

#[tokio::test]
async fn test_builder_with_signer() {
    // Known test private key (DO NOT use in production)
    let signer = PrivateKeySigner::new(
        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
    );
    let expected_addr = signer.address();

    let client = AraClient::builder()
        .signer(signer)
        .build_in_memory()
        .await
        .unwrap();
    assert_eq!(client.wallet_address(), Some(expected_addr));
}

// ─── Type formatting tests ──────────────────────────────────────────────────

#[test]
fn test_format_wei_zero() {
    assert_eq!(format_wei(U256::ZERO), "0.0");
}

#[test]
fn test_format_wei_one_eth() {
    let one_eth = U256::from(1_000_000_000_000_000_000u64);
    assert_eq!(format_wei(one_eth), "1.0");
}

#[test]
fn test_format_wei_fractional() {
    let half_eth = U256::from(500_000_000_000_000_000u64);
    assert_eq!(format_wei(half_eth), "0.5");
}

#[test]
fn test_format_wei_small() {
    let one_wei = U256::from(1u64);
    assert_eq!(format_wei(one_wei), "0.000000000000000001");
}

#[test]
fn test_format_token_amount_usdc() {
    // 100.5 USDC = 100_500_000 (6 decimals)
    let amount = U256::from(100_500_000u64);
    assert_eq!(format_token_amount(amount, 6), "100.5");
}

#[test]
fn test_format_token_amount_zero_decimals() {
    let amount = U256::from(42u64);
    assert_eq!(format_token_amount(amount, 0), "42.0");
}

// ─── Parse amount tests ─────────────────────────────────────────────────────

#[test]
fn test_parse_amount_whole() {
    let result = parse_amount("100", 18).unwrap();
    assert_eq!(result, U256::from(100_000_000_000_000_000_000u128));
}

#[test]
fn test_parse_amount_fractional() {
    let result = parse_amount("1.5", 18).unwrap();
    assert_eq!(result, U256::from(1_500_000_000_000_000_000u128));
}

#[test]
fn test_parse_amount_usdc() {
    let result = parse_amount("100.5", 6).unwrap();
    assert_eq!(result, U256::from(100_500_000u64));
}

#[test]
fn test_parse_amount_too_many_decimals() {
    let result = parse_amount("1.1234567", 6);
    assert!(result.is_err());
}

#[test]
fn test_parse_amount_invalid() {
    let result = parse_amount("1.2.3", 18);
    assert!(result.is_err());
}

// ─── Hex encode test ────────────────────────────────────────────────────────

#[test]
fn test_hex_encode() {
    let data = [0xde, 0xad, 0xbe, 0xef];
    assert_eq!(hex_encode(&data), "0xdeadbeef");
}

// ─── Calldata generation tests ──────────────────────────────────────────────

#[tokio::test]
async fn test_prepare_stake_calldata() {
    let client = AraClient::builder().build_in_memory().await.unwrap();
    let txs = client.staking().prepare_stake("100").unwrap();

    assert_eq!(txs.len(), 2);
    assert!(txs[0].description.contains("Approve"));
    assert!(txs[0].description.contains("ARA"));
    assert!(txs[1].description.contains("Stake"));
    assert!(txs[1].data.starts_with("0x"));
    assert_eq!(txs[0].value, "0x0");
    assert_eq!(txs[1].value, "0x0");
}

#[tokio::test]
async fn test_prepare_unstake_calldata() {
    let client = AraClient::builder().build_in_memory().await.unwrap();
    let txs = client.staking().prepare_unstake("50").unwrap();

    assert_eq!(txs.len(), 1);
    assert!(txs[0].description.contains("Unstake"));
}

#[tokio::test]
async fn test_prepare_stake_for_content() {
    let client = AraClient::builder().build_in_memory().await.unwrap();
    let content_id = FixedBytes::<32>::ZERO;
    let txs = client
        .staking()
        .prepare_stake_for_content(content_id, "10")
        .unwrap();

    assert_eq!(txs.len(), 2);
    assert!(txs[0].description.contains("Approve"));
    assert!(txs[1].description.contains("Stake"));
    assert!(txs[1].description.contains("content"));
}

#[tokio::test]
async fn test_prepare_publish_calldata() {
    let client = AraClient::builder().build_in_memory().await.unwrap();
    let content_hash = FixedBytes::<32>::ZERO;
    let result = client
        .content()
        .prepare_publish(
            content_hash,
            r#"{"title":"Test","description":"A test"}"#.to_string(),
            "0.01",
            1024,
            100,
            500,
            None,
        )
        .await
        .unwrap();

    assert_eq!(result.transactions.len(), 1);
    assert!(result.transactions[0].description.contains("Publish"));
    assert!(result.transactions[0].description.contains("ETH"));
    assert!(result.transactions[0].data.starts_with("0x"));
}

#[tokio::test]
async fn test_prepare_publish_with_token() {
    let mut config = AppConfig::default();
    config.ethereum.supported_tokens.push(ara_core::config::TokenConfig {
        address: "0x1234567890abcdef1234567890abcdef12345678".to_string(),
        symbol: "USDC".to_string(),
        decimals: 6,
    });

    let client = AraClient::builder()
        .config(config)
        .build_in_memory()
        .await
        .unwrap();

    let token_addr: Address = "0x1234567890abcdef1234567890abcdef12345678"
        .parse()
        .unwrap();
    let result = client
        .content()
        .prepare_publish(
            FixedBytes::<32>::ZERO,
            r#"{"title":"Token-priced"}"#.to_string(),
            "9.99",
            2048,
            50,
            250,
            Some(token_addr),
        )
        .await
        .unwrap();

    assert!(result.transactions[0].description.contains("USDC"));
}

#[tokio::test]
async fn test_prepare_delist_calldata() {
    let client = AraClient::builder().build_in_memory().await.unwrap();
    let txs = client
        .content()
        .prepare_delist(FixedBytes::<32>::ZERO)
        .unwrap();

    assert_eq!(txs.len(), 1);
    assert!(txs[0].description.contains("Delist"));
}

// ─── Collection calldata tests ──────────────────────────────────────────────

#[tokio::test]
async fn test_prepare_create_collection() {
    let client = AraClient::builder().build_in_memory().await.unwrap();
    let txs = client
        .collections()
        .prepare_create("My Collection", "A test collection", "https://example.com/banner.png")
        .unwrap();

    assert_eq!(txs.len(), 1);
    assert!(txs[0].description.contains("Create collection"));
}

#[tokio::test]
async fn test_prepare_delete_collection() {
    let client = AraClient::builder().build_in_memory().await.unwrap();
    let txs = client
        .collections()
        .prepare_delete(U256::from(1))
        .unwrap();

    assert_eq!(txs.len(), 1);
    assert!(txs[0].description.contains("Delete"));
}

#[tokio::test]
async fn test_prepare_add_item_to_collection() {
    let client = AraClient::builder().build_in_memory().await.unwrap();
    let txs = client
        .collections()
        .prepare_add_item(U256::from(1), FixedBytes::<32>::ZERO)
        .unwrap();

    assert_eq!(txs.len(), 1);
    assert!(txs[0].description.contains("Add item"));
}

// ─── Name registry calldata tests ───────────────────────────────────────────

#[tokio::test]
async fn test_prepare_register_name() {
    let client = AraClient::builder().build_in_memory().await.unwrap();
    let txs = client.names().prepare_register("alice").unwrap();

    assert_eq!(txs.len(), 1);
    assert!(txs[0].description.contains("Register"));
    assert!(txs[0].description.contains("alice"));
}

#[tokio::test]
async fn test_prepare_remove_name() {
    let client = AraClient::builder().build_in_memory().await.unwrap();
    let txs = client.names().prepare_remove().unwrap();

    assert_eq!(txs.len(), 1);
    assert!(txs[0].description.contains("Remove"));
}

// ─── Moderation calldata tests ──────────────────────────────────────────────

#[tokio::test]
async fn test_prepare_flag_content() {
    let mut config = AppConfig::default();
    config.ethereum.moderation_address =
        "0x0000000000000000000000000000000000000001".to_string();

    let client = AraClient::builder()
        .config(config)
        .build_in_memory()
        .await
        .unwrap();

    let txs = client
        .moderation()
        .prepare_flag(FixedBytes::<32>::ZERO, 0, false)
        .unwrap();

    assert_eq!(txs.len(), 1);
    assert!(txs[0].description.contains("Flag"));
    assert!(!txs[0].description.contains("emergency"));
}

#[tokio::test]
async fn test_prepare_emergency_flag() {
    let mut config = AppConfig::default();
    config.ethereum.moderation_address =
        "0x0000000000000000000000000000000000000001".to_string();

    let client = AraClient::builder()
        .config(config)
        .build_in_memory()
        .await
        .unwrap();

    let txs = client
        .moderation()
        .prepare_flag(FixedBytes::<32>::ZERO, 4, true)
        .unwrap();

    assert!(txs[0].description.contains("emergency"));
}

#[tokio::test]
async fn test_prepare_vote() {
    let mut config = AppConfig::default();
    config.ethereum.moderation_address =
        "0x0000000000000000000000000000000000000001".to_string();

    let client = AraClient::builder()
        .config(config)
        .build_in_memory()
        .await
        .unwrap();

    let txs = client
        .moderation()
        .prepare_vote(FixedBytes::<32>::ZERO, true)
        .unwrap();

    assert!(txs[0].description.contains("uphold"));
}

#[tokio::test]
async fn test_prepare_set_nsfw() {
    let mut config = AppConfig::default();
    config.ethereum.moderation_address =
        "0x0000000000000000000000000000000000000001".to_string();

    let client = AraClient::builder()
        .config(config)
        .build_in_memory()
        .await
        .unwrap();

    let txs = client
        .moderation()
        .prepare_set_nsfw(FixedBytes::<32>::ZERO, true)
        .unwrap();

    assert!(txs[0].description.contains("Set NSFW"));
}

// ─── Marketplace calldata tests ─────────────────────────────────────────────

#[tokio::test]
async fn test_prepare_list_for_resale() {
    let client = AraClient::builder().build_in_memory().await.unwrap();
    let txs = client
        .marketplace()
        .prepare_list_for_resale(FixedBytes::<32>::ZERO, U256::from(1_000_000_000_000_000_000u128))
        .unwrap();

    assert_eq!(txs.len(), 2);
    assert!(txs[0].description.contains("Approve"));
    assert!(txs[1].description.contains("resale"));
}

#[tokio::test]
async fn test_prepare_cancel_listing() {
    let client = AraClient::builder().build_in_memory().await.unwrap();
    let txs = client
        .marketplace()
        .prepare_cancel_listing(FixedBytes::<32>::ZERO)
        .unwrap();

    assert_eq!(txs.len(), 1);
    assert!(txs[0].description.contains("Cancel"));
}

#[tokio::test]
async fn test_prepare_buy_resale() {
    let client = AraClient::builder().build_in_memory().await.unwrap();
    let price = U256::from(500_000_000_000_000_000u128);
    let txs = client
        .marketplace()
        .prepare_buy_resale(FixedBytes::<32>::ZERO, Address::ZERO, price)
        .unwrap();

    assert_eq!(txs.len(), 1);
    assert!(txs[0].description.contains("Buy resale"));
    assert_ne!(txs[0].value, "0x0"); // ETH value should be nonzero
}

// ─── DB-backed content queries ──────────────────────────────────────────────

#[tokio::test]
async fn test_search_empty_db() {
    let client = AraClient::builder().build_in_memory().await.unwrap();
    let results = client.content().search("", 10).await.unwrap();
    assert!(results.is_empty());
}

#[tokio::test]
async fn test_get_content_not_found() {
    let client = AraClient::builder().build_in_memory().await.unwrap();
    let result = client.content().get_content("0x0000").await.unwrap();
    assert!(result.is_none());
}

// ─── Analytics on empty DB ──────────────────────────────────────────────────

#[tokio::test]
async fn test_analytics_empty_db() {
    let client = AraClient::builder().build_in_memory().await.unwrap();

    let analytics = client
        .analytics()
        .get_item_analytics("0x0000")
        .await
        .unwrap();
    assert_eq!(analytics.total_sales, 0);
    assert_eq!(analytics.unique_buyers, 0);

    let collectors = client.analytics().get_top_collectors(10).await.unwrap();
    assert!(collectors.is_empty());

    let overview = client.analytics().get_overview().await.unwrap();
    assert_eq!(overview.total_sales, 0);
    assert_eq!(overview.total_items, 0);
}

// ─── Signer address derivation ──────────────────────────────────────────────

#[test]
fn test_private_key_signer_address() {
    // Hardhat account #0
    let signer = PrivateKeySigner::new(
        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
    );
    // Known address for this key
    assert_eq!(
        signer.address(),
        address!("f39Fd6e51aad88F6F4ce6aB8827279cffFb92266")
    );
}

#[test]
fn test_private_key_signer_invalid_key() {
    let signer = PrivateKeySigner::new("0xDEAD");
    assert_eq!(signer.address(), Address::ZERO);
}
