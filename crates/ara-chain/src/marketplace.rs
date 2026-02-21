use alloy::primitives::{Address, FixedBytes, U256};
use alloy::providers::Provider;
use alloy::sol_types::SolCall;
use anyhow::Result;
use tracing::info;

use crate::contracts::IMarketplace;

/// Wrapper for Marketplace contract interactions (purchase, rewards).
pub struct MarketplaceClient<P> {
    address: Address,
    provider: P,
}

impl<P: Provider + Clone> MarketplaceClient<P> {
    pub fn new(address: Address, provider: P) -> Self {
        Self { address, provider }
    }

    // --- Read operations ---

    /// Check if a buyer has purchased specific content.
    pub async fn has_purchased(
        &self,
        content_id: FixedBytes<32>,
        buyer: Address,
    ) -> Result<bool> {
        info!("Checking purchase: buyer={}, content={}", buyer, content_id);
        let contract = IMarketplace::new(self.address, &self.provider);
        let result = contract.hasPurchased(content_id, buyer).call().await?;
        Ok(result)
    }

    /// Get the reward pool balance (in wei) for a content item.
    pub async fn reward_pool(&self, content_id: FixedBytes<32>) -> Result<U256> {
        let contract = IMarketplace::new(self.address, &self.provider);
        let result = contract.rewardPool(content_id).call().await?;
        Ok(result)
    }

    /// Get claimable rewards (in wei) for a seeder.
    pub async fn claimable_rewards(&self, seeder: Address) -> Result<U256> {
        info!("Querying claimable rewards for {}", seeder);
        let contract = IMarketplace::new(self.address, &self.provider);
        let result = contract.claimableRewards(seeder).call().await?;
        Ok(result)
    }

    /// Get the timestamp of the last purchase for a content item.
    pub async fn last_purchase_time(&self, content_id: FixedBytes<32>) -> Result<U256> {
        let contract = IMarketplace::new(self.address, &self.provider);
        let result = contract.lastPurchaseTime(content_id).call().await?;
        Ok(result)
    }

    /// Get the distribution window duration in seconds.
    pub async fn distribution_window(&self) -> Result<U256> {
        let contract = IMarketplace::new(self.address, &self.provider);
        let result = contract.distributionWindow().call().await?;
        Ok(result)
    }

    /// Get the marketplace contract address.
    pub fn address(&self) -> Address {
        self.address
    }
}

// Calldata encoding — no provider needed.
impl<P> MarketplaceClient<P> {
    /// Encode calldata for `purchase(contentId)`.
    /// The frontend sends this with the correct ETH value attached.
    pub fn purchase_calldata(content_id: FixedBytes<32>) -> Vec<u8> {
        IMarketplace::purchaseCall {
            contentId: content_id,
        }
        .abi_encode()
    }

    /// Encode calldata for `claimRewards()`.
    pub fn claim_rewards_calldata() -> Vec<u8> {
        IMarketplace::claimRewardsCall {}.abi_encode()
    }

    /// Encode calldata for `distributeRewards(contentId, seeders, weights)`.
    /// This is called by the reward reporter (admin operation).
    pub fn distribute_rewards_calldata(
        content_id: FixedBytes<32>,
        seeders: Vec<Address>,
        weights: Vec<U256>,
    ) -> Vec<u8> {
        IMarketplace::distributeRewardsCall {
            contentId: content_id,
            seeders,
            weights,
        }
        .abi_encode()
    }

    /// Encode calldata for `publicDistributeWithProofs(contentId, bundles)`.
    /// Bundles contain buyer-signed EIP-712 receipts for each seeder.
    /// This is the trustless fallback when the creator hasn't distributed within the window.
    pub fn public_distribute_calldata(
        content_id: FixedBytes<32>,
        bundles: Vec<IMarketplace::SeederBundle>,
    ) -> Vec<u8> {
        IMarketplace::publicDistributeWithProofsCall {
            contentId: content_id,
            bundles,
        }
        .abi_encode()
    }
}
