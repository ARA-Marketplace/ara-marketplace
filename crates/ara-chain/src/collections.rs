use alloy::primitives::{Address, FixedBytes, U256};
use alloy::providers::Provider;
use alloy::sol_types::SolCall;
use anyhow::Result;

use crate::contracts::IAraCollections;

/// Wrapper for AraCollections contract interactions.
pub struct CollectionsClient<P> {
    address: Address,
    provider: P,
}

impl<P: Provider + Clone> CollectionsClient<P> {
    pub fn new(address: Address, provider: P) -> Self {
        Self { address, provider }
    }

    pub fn address(&self) -> Address {
        self.address
    }

    /// Get collection metadata from chain.
    pub async fn get_collection(
        &self,
        collection_id: U256,
    ) -> Result<(Address, String, String, String, U256, bool)> {
        let contract = IAraCollections::new(self.address, &self.provider);
        let result = contract.collections(collection_id).call().await?;
        Ok((result.creator, result.name, result.description, result.bannerUri, result.createdAt, result.active))
    }

    /// Get all content IDs in a collection.
    pub async fn get_collection_items(
        &self,
        collection_id: U256,
    ) -> Result<Vec<FixedBytes<32>>> {
        let contract = IAraCollections::new(self.address, &self.provider);
        let result = contract.getCollectionItems(collection_id).call().await?;
        Ok(result)
    }

    /// Get all collection IDs for a creator.
    pub async fn get_creator_collections(&self, creator: Address) -> Result<Vec<U256>> {
        let contract = IAraCollections::new(self.address, &self.provider);
        let result = contract.getCreatorCollections(creator).call().await?;
        Ok(result)
    }

    /// Get which collection a content item belongs to (0 = none).
    pub async fn content_collection(&self, content_id: FixedBytes<32>) -> Result<U256> {
        let contract = IAraCollections::new(self.address, &self.provider);
        let result = contract.contentCollection(content_id).call().await?;
        Ok(result)
    }

    /// Get the next collection ID that will be assigned.
    pub async fn next_collection_id(&self) -> Result<U256> {
        let contract = IAraCollections::new(self.address, &self.provider);
        let result = contract.nextCollectionId().call().await?;
        Ok(result)
    }
}

// Calldata encoding — no provider needed.
impl<P> CollectionsClient<P> {
    pub fn create_collection_calldata(name: &str, description: &str, banner_uri: &str) -> Vec<u8> {
        IAraCollections::createCollectionCall {
            name: name.to_string(),
            description: description.to_string(),
            bannerUri: banner_uri.to_string(),
        }
        .abi_encode()
    }

    pub fn update_collection_calldata(
        collection_id: U256,
        name: &str,
        description: &str,
        banner_uri: &str,
    ) -> Vec<u8> {
        IAraCollections::updateCollectionCall {
            collectionId: collection_id,
            name: name.to_string(),
            description: description.to_string(),
            bannerUri: banner_uri.to_string(),
        }
        .abi_encode()
    }

    pub fn delete_collection_calldata(collection_id: U256) -> Vec<u8> {
        IAraCollections::deleteCollectionCall {
            collectionId: collection_id,
        }
        .abi_encode()
    }

    pub fn add_item_calldata(collection_id: U256, content_id: FixedBytes<32>) -> Vec<u8> {
        IAraCollections::addItemCall {
            collectionId: collection_id,
            contentId: content_id,
        }
        .abi_encode()
    }

    pub fn remove_item_calldata(collection_id: U256, content_id: FixedBytes<32>) -> Vec<u8> {
        IAraCollections::removeItemCall {
            collectionId: collection_id,
            contentId: content_id,
        }
        .abi_encode()
    }
}
