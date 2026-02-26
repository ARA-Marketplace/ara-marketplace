use alloy::primitives::{Address, U256};
use alloy::providers::{Provider, ProviderBuilder};
use anyhow::Result;

use crate::content_token::ContentTokenClient;
use crate::events::EventIndexer;
use crate::marketplace::MarketplaceClient;
use crate::staking::StakingClient;
use crate::token::TokenClient;

/// Contract addresses for the Ara Marketplace deployment.
#[derive(Debug, Clone)]
pub struct ContractAddresses {
    pub ara_token: Address,
    pub staking: Address,
    pub registry: Address,
    pub marketplace: Address,
}

/// Main entry point for all Ethereum interactions.
/// Wraps a shared provider and creates typed clients for each contract.
pub struct AraChain<P> {
    provider: P,
    pub token: TokenClient<P>,
    pub staking: StakingClient<P>,
    pub registry: ContentTokenClient<P>,
    pub marketplace: MarketplaceClient<P>,
    pub events: EventIndexer<P>,
}

impl<P: Provider + Clone> AraChain<P> {
    pub fn new(provider: P, addresses: ContractAddresses) -> Self {
        Self {
            token: TokenClient::new(addresses.ara_token, provider.clone()),
            staking: StakingClient::new(addresses.staking, provider.clone()),
            registry: ContentTokenClient::new(addresses.registry, provider.clone()),
            marketplace: MarketplaceClient::new(addresses.marketplace, provider.clone()),
            events: EventIndexer::new(
                addresses.registry,
                addresses.marketplace,
                addresses.staking,
                provider.clone(),
            ),
            provider,
        }
    }

    /// Get the native ETH balance for an address.
    pub async fn get_eth_balance(&self, address: Address) -> Result<U256> {
        let balance = self.provider.get_balance(address).await?;
        Ok(balance)
    }

    /// Get the latest block number.
    pub async fn get_block_number(&self) -> Result<u64> {
        Ok(self.provider.get_block_number().await?)
    }

    /// Get the contract addresses this client was configured with.
    pub fn token_address(&self) -> Address {
        self.token.address()
    }

    pub fn staking_address(&self) -> Address {
        self.staking.address()
    }

    pub fn registry_address(&self) -> Address {
        self.registry.address()
    }

    pub fn marketplace_address(&self) -> Address {
        self.marketplace.address()
    }
}

/// Connect to Ethereum via HTTP RPC and create an AraChain client.
pub fn connect_http(
    rpc_url: &str,
    addresses: ContractAddresses,
) -> Result<AraChain<impl Provider + Clone>> {
    let provider = ProviderBuilder::new().connect_http(rpc_url.parse()?);
    Ok(AraChain::new(provider, addresses))
}
