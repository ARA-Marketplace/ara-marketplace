use alloy::primitives::{Address, FixedBytes, U256};
use alloy::providers::Provider;
use alloy::sol_types::SolCall;
use anyhow::Result;

use crate::contracts::IContentRegistry;

/// Wrapper for ContentRegistry contract interactions.
/// Handles content publishing, updates, and queries.
pub struct RegistryClient<P> {
    address: Address,
    provider: P,
}

impl<P: Provider + Clone> RegistryClient<P> {
    pub fn new(address: Address, provider: P) -> Self {
        Self { address, provider }
    }

    // --- Read operations ---

    /// Get the total number of published content items.
    pub async fn get_content_count(&self) -> Result<U256> {
        let contract = IContentRegistry::new(self.address, &self.provider);
        let result = contract.getContentCount().call().await?;
        Ok(result)
    }

    /// Get the BLAKE3 content hash for a content ID.
    pub async fn get_content_hash(
        &self,
        content_id: FixedBytes<32>,
    ) -> Result<FixedBytes<32>> {
        let contract = IContentRegistry::new(self.address, &self.provider);
        let result = contract.getContentHash(content_id).call().await?;
        Ok(result)
    }

    /// Get the price (in wei) for a content item.
    pub async fn get_price(&self, content_id: FixedBytes<32>) -> Result<U256> {
        let contract = IContentRegistry::new(self.address, &self.provider);
        let result = contract.getPrice(content_id).call().await?;
        Ok(result)
    }

    /// Get the creator address for a content item.
    pub async fn get_creator(&self, content_id: FixedBytes<32>) -> Result<Address> {
        let contract = IContentRegistry::new(self.address, &self.provider);
        let result = contract.getCreator(content_id).call().await?;
        Ok(result)
    }

    /// Check if a content item is currently active (not delisted).
    pub async fn is_active(&self, content_id: FixedBytes<32>) -> Result<bool> {
        let contract = IContentRegistry::new(self.address, &self.provider);
        let result = contract.isActive(content_id).call().await?;
        Ok(result)
    }

    /// Get the registry contract address.
    pub fn address(&self) -> Address {
        self.address
    }
}

// Calldata encoding — no provider needed.
impl<P> RegistryClient<P> {
    /// Encode calldata for `publishContent(contentHash, metadataURI, priceWei)`.
    pub fn publish_content_calldata(
        content_hash: FixedBytes<32>,
        metadata_uri: String,
        price_wei: U256,
    ) -> Vec<u8> {
        IContentRegistry::publishContentCall {
            contentHash: content_hash,
            metadataURI: metadata_uri,
            priceWei: price_wei,
        }
        .abi_encode()
    }

    /// Encode calldata for `updateContent(contentId, newPriceWei, newMetadataURI)`.
    pub fn update_content_calldata(
        content_id: FixedBytes<32>,
        new_price_wei: U256,
        new_metadata_uri: String,
    ) -> Vec<u8> {
        IContentRegistry::updateContentCall {
            contentId: content_id,
            newPriceWei: new_price_wei,
            newMetadataURI: new_metadata_uri,
        }
        .abi_encode()
    }

    /// Encode calldata for `updateContentFile(contentId, newContentHash)`.
    /// Used to replace the P2P blob for an existing listing without changing the contentId.
    pub fn update_content_file_calldata(
        content_id: FixedBytes<32>,
        new_content_hash: FixedBytes<32>,
    ) -> Vec<u8> {
        IContentRegistry::updateContentFileCall {
            contentId: content_id,
            newContentHash: new_content_hash,
        }
        .abi_encode()
    }

    /// Encode calldata for `delistContent(contentId)`.
    pub fn delist_content_calldata(content_id: FixedBytes<32>) -> Vec<u8> {
        IContentRegistry::delistContentCall {
            contentId: content_id,
        }
        .abi_encode()
    }
}
