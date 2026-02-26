use alloy::primitives::{Address, FixedBytes, Uint, U256};
use alloy::providers::Provider;
use alloy::sol_types::SolCall;
use anyhow::Result;

use crate::contracts::IAraContent;

/// Wrapper for AraContent (ERC-1155) contract interactions.
/// Handles content publishing, updates, and queries.
pub struct ContentTokenClient<P> {
    address: Address,
    provider: P,
}

impl<P: Provider + Clone> ContentTokenClient<P> {
    pub fn new(address: Address, provider: P) -> Self {
        Self { address, provider }
    }

    // --- Read operations ---

    /// Get the total number of published content items.
    pub async fn get_content_count(&self) -> Result<U256> {
        let contract = IAraContent::new(self.address, &self.provider);
        let result = contract.getContentCount().call().await?;
        Ok(result)
    }

    /// Get the BLAKE3 content hash for a content ID.
    pub async fn get_content_hash(
        &self,
        content_id: FixedBytes<32>,
    ) -> Result<FixedBytes<32>> {
        let contract = IAraContent::new(self.address, &self.provider);
        let result = contract.getContentHash(content_id).call().await?;
        Ok(result)
    }

    /// Get the price (in wei) for a content item.
    pub async fn get_price(&self, content_id: FixedBytes<32>) -> Result<U256> {
        let contract = IAraContent::new(self.address, &self.provider);
        let result = contract.getPrice(content_id).call().await?;
        Ok(result)
    }

    /// Get the creator address for a content item.
    pub async fn get_creator(&self, content_id: FixedBytes<32>) -> Result<Address> {
        let contract = IAraContent::new(self.address, &self.provider);
        let result = contract.getCreator(content_id).call().await?;
        Ok(result)
    }

    /// Check if a content item is currently active (not delisted).
    pub async fn is_active(&self, content_id: FixedBytes<32>) -> Result<bool> {
        let contract = IAraContent::new(self.address, &self.provider);
        let result = contract.isActive(content_id).call().await?;
        Ok(result)
    }

    /// Get the maximum supply for a content item (0 = unlimited).
    pub async fn get_max_supply(&self, content_id: FixedBytes<32>) -> Result<U256> {
        let contract = IAraContent::new(self.address, &self.provider);
        let result = contract.getMaxSupply(content_id).call().await?;
        Ok(result)
    }

    /// Get the total number of tokens minted for a content item.
    pub async fn get_total_minted(&self, content_id: FixedBytes<32>) -> Result<U256> {
        let contract = IAraContent::new(self.address, &self.provider);
        let result = contract.getTotalMinted(content_id).call().await?;
        Ok(result)
    }

    /// Check if an operator is approved for all tokens of an account.
    pub async fn is_approved_for_all(
        &self,
        account: Address,
        operator: Address,
    ) -> Result<bool> {
        let contract = IAraContent::new(self.address, &self.provider);
        let result = contract.isApprovedForAll(account, operator).call().await?;
        Ok(result)
    }

    /// Get the content token contract address.
    pub fn address(&self) -> Address {
        self.address
    }
}

// Calldata encoding — no provider needed.
impl<P> ContentTokenClient<P> {
    /// Encode calldata for `publishContent(contentHash, metadataURI, priceWei, fileSize, maxSupply, royaltyBps)`.
    pub fn publish_content_calldata(
        content_hash: FixedBytes<32>,
        metadata_uri: String,
        price_wei: U256,
        file_size: U256,
        max_supply: U256,
        royalty_bps: u128,
    ) -> Vec<u8> {
        IAraContent::publishContentCall {
            contentHash: content_hash,
            metadataURI: metadata_uri,
            priceWei: price_wei,
            fileSize: file_size,
            maxSupply: max_supply,
            royaltyBps: Uint::<96, 2>::from(royalty_bps),
        }
        .abi_encode()
    }

    /// Encode calldata for `updateContent(contentId, newPriceWei, newMetadataURI)`.
    pub fn update_content_calldata(
        content_id: FixedBytes<32>,
        new_price_wei: U256,
        new_metadata_uri: String,
    ) -> Vec<u8> {
        IAraContent::updateContentCall {
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
        IAraContent::updateContentFileCall {
            contentId: content_id,
            newContentHash: new_content_hash,
        }
        .abi_encode()
    }

    /// Encode calldata for `delistContent(contentId)`.
    pub fn delist_content_calldata(content_id: FixedBytes<32>) -> Vec<u8> {
        IAraContent::delistContentCall {
            contentId: content_id,
        }
        .abi_encode()
    }

    /// Encode calldata for `setApprovalForAll(operator, approved)`.
    pub fn set_approval_for_all_calldata(operator: Address, approved: bool) -> Vec<u8> {
        IAraContent::setApprovalForAllCall { operator, approved }.abi_encode()
    }
}
