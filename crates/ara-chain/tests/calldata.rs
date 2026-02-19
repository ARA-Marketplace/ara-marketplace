use alloy::primitives::{address, fixed_bytes, Address, U256};
use alloy::sol_types::SolCall;
use ara_chain::contracts::{IAraStaking, IAraToken, IContentRegistry, IMarketplace};
use ara_chain::marketplace::MarketplaceClient;
use ara_chain::registry::RegistryClient;
use ara_chain::staking::StakingClient;
use ara_chain::token::TokenClient;

/// Verify approve calldata matches ABI spec: selector + encoded (address, uint256).
#[test]
fn test_approve_calldata() {
    let spender = address!("0000000000000000000000000000000000000001");
    let amount = U256::from(1000u64);

    let calldata = TokenClient::<()>::approve_calldata(spender, amount);

    // First 4 bytes: function selector for approve(address,uint256)
    assert_eq!(&calldata[..4], IAraToken::approveCall::SELECTOR.as_slice());
    assert_eq!(calldata.len(), 4 + 64); // selector + 2 params (32 bytes each)

    // Verify round-trip: decode the full calldata (selector + params)
    let decoded = IAraToken::approveCall::abi_decode(&calldata).unwrap();
    assert_eq!(decoded.spender, spender);
    assert_eq!(decoded.amount, amount);
}

/// Verify transfer calldata encoding.
#[test]
fn test_transfer_calldata() {
    let to = address!("0000000000000000000000000000000000000002");
    let amount = U256::from(500u64);

    let calldata = TokenClient::<()>::transfer_calldata(to, amount);
    let decoded = IAraToken::transferCall::abi_decode(&calldata).unwrap();
    assert_eq!(decoded.to, to);
    assert_eq!(decoded.amount, amount);
}

/// Verify stake calldata encoding.
#[test]
fn test_stake_calldata() {
    let amount = U256::from(10_000u64);

    let calldata = StakingClient::<()>::stake_calldata(amount);
    let decoded = IAraStaking::stakeCall::abi_decode(&calldata).unwrap();
    assert_eq!(decoded.amount, amount);
}

/// Verify unstake calldata encoding.
#[test]
fn test_unstake_calldata() {
    let amount = U256::from(5_000u64);

    let calldata = StakingClient::<()>::unstake_calldata(amount);
    let decoded = IAraStaking::unstakeCall::abi_decode(&calldata).unwrap();
    assert_eq!(decoded.amount, amount);
}

/// Verify stakeForContent calldata encoding.
#[test]
fn test_stake_for_content_calldata() {
    let content_id =
        fixed_bytes!("abcdef0000000000000000000000000000000000000000000000000000000001");
    let amount = U256::from(2_000u64);

    let calldata = StakingClient::<()>::stake_for_content_calldata(content_id, amount);
    let decoded = IAraStaking::stakeForContentCall::abi_decode(&calldata).unwrap();
    assert_eq!(decoded.contentId, content_id);
    assert_eq!(decoded.amount, amount);
}

/// Verify publishContent calldata encoding (includes string parameter).
#[test]
fn test_publish_content_calldata() {
    let content_hash =
        fixed_bytes!("1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef");
    let metadata_uri = "ipfs://QmTest123".to_string();
    let price_wei = U256::from(100_000_000_000_000_000u128); // 0.1 ETH

    let calldata = RegistryClient::<()>::publish_content_calldata(
        content_hash,
        metadata_uri.clone(),
        price_wei,
    );

    // Verify selector
    assert_eq!(
        &calldata[..4],
        IContentRegistry::publishContentCall::SELECTOR.as_slice()
    );

    // Verify round-trip
    let decoded = IContentRegistry::publishContentCall::abi_decode(&calldata).unwrap();
    assert_eq!(decoded.contentHash, content_hash);
    assert_eq!(decoded.metadataURI, metadata_uri);
    assert_eq!(decoded.priceWei, price_wei);
}

/// Verify purchase calldata encoding.
#[test]
fn test_purchase_calldata() {
    let content_id =
        fixed_bytes!("aabbccdd00000000000000000000000000000000000000000000000000000000");

    let calldata = MarketplaceClient::<()>::purchase_calldata(content_id);
    let decoded = IMarketplace::purchaseCall::abi_decode(&calldata).unwrap();
    assert_eq!(decoded.contentId, content_id);
}

/// Verify claimRewards calldata encoding (no parameters).
#[test]
fn test_claim_rewards_calldata() {
    let calldata = MarketplaceClient::<()>::claim_rewards_calldata();
    // claimRewards() has no parameters, just a 4-byte selector
    assert_eq!(calldata.len(), 4);
}

/// Verify distributeRewards calldata encoding (complex: arrays).
#[test]
fn test_distribute_rewards_calldata() {
    let content_id =
        fixed_bytes!("1111111111111111111111111111111111111111111111111111111111111111");
    let seeders = vec![
        address!("0000000000000000000000000000000000000001"),
        address!("0000000000000000000000000000000000000002"),
    ];
    let weights = vec![U256::from(70u64), U256::from(30u64)];

    let calldata = MarketplaceClient::<()>::distribute_rewards_calldata(
        content_id,
        seeders.clone(),
        weights.clone(),
    );

    let decoded = IMarketplace::distributeRewardsCall::abi_decode(&calldata).unwrap();
    assert_eq!(decoded.contentId, content_id);
    assert_eq!(decoded.seeders, seeders);
    assert_eq!(decoded.weights, weights);
}

/// Verify ContractAddresses with real ARA token address.
#[test]
fn test_contract_addresses() {
    use ara_chain::ContractAddresses;

    let addresses = ContractAddresses {
        ara_token: address!("a92e7c82b11d10716ab534051b271d2f6aef7df5"),
        staking: Address::ZERO,
        registry: Address::ZERO,
        marketplace: Address::ZERO,
    };

    assert_eq!(
        addresses.ara_token,
        address!("a92e7c82b11d10716ab534051b271d2f6aef7df5")
    );
}
