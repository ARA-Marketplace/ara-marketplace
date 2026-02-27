use alloy::primitives::{Address, FixedBytes, U256};
use alloy::providers::Provider;
use alloy::sol_types::SolCall;
use anyhow::Result;
use tracing::info;

use crate::contracts::IMarketplace;

/// Wrapper for Marketplace contract interactions (purchase, per-receipt reward claiming, resales).
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

    /// Get the remaining (unclaimed) reward for a buyer's purchase.
    pub async fn get_buyer_reward(
        &self,
        content_id: FixedBytes<32>,
        buyer: Address,
    ) -> Result<U256> {
        let contract = IMarketplace::new(self.address, &self.provider);
        let result = contract.getBuyerReward(content_id, buyer).call().await?;
        Ok(result)
    }

    /// Read a resale listing from the on-chain mapping.
    /// Returns (price, active). price is 0 and active is false if no listing exists.
    pub async fn get_listing(
        &self,
        content_id: FixedBytes<32>,
        seller: Address,
    ) -> Result<(U256, bool)> {
        let contract = IMarketplace::new(self.address, &self.provider);
        let result = contract.listings(content_id, seller).call().await?;
        Ok((result.price, result.active))
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

    /// Encode calldata for `claimDeliveryRewards(claims)`.
    /// Batch claim: submit multiple delivery receipts in one transaction.
    pub fn claim_delivery_rewards_calldata(
        claims: Vec<IMarketplace::ClaimParams>,
    ) -> Vec<u8> {
        IMarketplace::claimDeliveryRewardsCall { claims }
            .abi_encode()
    }

    /// Encode calldata for `listForResale(contentId, price)`.
    pub fn list_for_resale_calldata(
        content_id: FixedBytes<32>,
        price: U256,
    ) -> Vec<u8> {
        IMarketplace::listForResaleCall {
            contentId: content_id,
            price,
        }
        .abi_encode()
    }

    /// Encode calldata for `cancelListing(contentId)`.
    pub fn cancel_listing_calldata(content_id: FixedBytes<32>) -> Vec<u8> {
        IMarketplace::cancelListingCall {
            contentId: content_id,
        }
        .abi_encode()
    }

    /// Encode calldata for `buyResale(contentId, seller)`.
    /// The frontend sends this with the listing price as ETH value.
    pub fn buy_resale_calldata(
        content_id: FixedBytes<32>,
        seller: Address,
    ) -> Vec<u8> {
        IMarketplace::buyResaleCall {
            contentId: content_id,
            seller,
        }
        .abi_encode()
    }
}
