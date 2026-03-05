use alloy::primitives::{Address, FixedBytes, U256};
use anyhow::Result;

use ara_chain::collections::CollectionsClient;

use crate::client::AraClient;
use crate::types::{hex_encode, TransactionRequest};

/// Collection operations: create, update, delete, manage items, query.
pub struct CollectionOps<'a> {
    pub(crate) client: &'a AraClient,
}

impl CollectionOps<'_> {
    /// Prepare a create-collection transaction.
    pub fn prepare_create(
        &self,
        name: &str,
        description: &str,
        banner_uri: &str,
    ) -> Result<Vec<TransactionRequest>> {
        let eth = &self.client.config.ethereum;
        let collections_addr: Address = eth.collections_address.parse()?;

        let calldata =
            CollectionsClient::<()>::create_collection_calldata(name, description, banner_uri);

        Ok(vec![TransactionRequest {
            to: format!("{collections_addr:#x}"),
            data: hex_encode(&calldata),
            value: "0x0".to_string(),
            description: format!("Create collection \"{}\"", name),
        }])
    }

    /// Prepare an update-collection transaction.
    pub fn prepare_update(
        &self,
        collection_id: U256,
        name: &str,
        description: &str,
        banner_uri: &str,
    ) -> Result<Vec<TransactionRequest>> {
        let eth = &self.client.config.ethereum;
        let collections_addr: Address = eth.collections_address.parse()?;

        let calldata = CollectionsClient::<()>::update_collection_calldata(
            collection_id,
            name,
            description,
            banner_uri,
        );

        Ok(vec![TransactionRequest {
            to: format!("{collections_addr:#x}"),
            data: hex_encode(&calldata),
            value: "0x0".to_string(),
            description: format!("Update collection \"{}\"", name),
        }])
    }

    /// Prepare a delete-collection transaction.
    pub fn prepare_delete(
        &self,
        collection_id: U256,
    ) -> Result<Vec<TransactionRequest>> {
        let eth = &self.client.config.ethereum;
        let collections_addr: Address = eth.collections_address.parse()?;

        let calldata = CollectionsClient::<()>::delete_collection_calldata(collection_id);

        Ok(vec![TransactionRequest {
            to: format!("{collections_addr:#x}"),
            data: hex_encode(&calldata),
            value: "0x0".to_string(),
            description: format!("Delete collection {}", collection_id),
        }])
    }

    /// Prepare an add-item-to-collection transaction.
    pub fn prepare_add_item(
        &self,
        collection_id: U256,
        content_id: FixedBytes<32>,
    ) -> Result<Vec<TransactionRequest>> {
        let eth = &self.client.config.ethereum;
        let collections_addr: Address = eth.collections_address.parse()?;

        let calldata = CollectionsClient::<()>::add_item_calldata(collection_id, content_id);

        Ok(vec![TransactionRequest {
            to: format!("{collections_addr:#x}"),
            data: hex_encode(&calldata),
            value: "0x0".to_string(),
            description: "Add item to collection".to_string(),
        }])
    }

    /// Prepare a remove-item-from-collection transaction.
    pub fn prepare_remove_item(
        &self,
        collection_id: U256,
        content_id: FixedBytes<32>,
    ) -> Result<Vec<TransactionRequest>> {
        let eth = &self.client.config.ethereum;
        let collections_addr: Address = eth.collections_address.parse()?;

        let calldata = CollectionsClient::<()>::remove_item_calldata(collection_id, content_id);

        Ok(vec![TransactionRequest {
            to: format!("{collections_addr:#x}"),
            data: hex_encode(&calldata),
            value: "0x0".to_string(),
            description: "Remove item from collection".to_string(),
        }])
    }

    /// Get collection metadata from on-chain.
    pub async fn get_collection(
        &self,
        collection_id: U256,
    ) -> Result<(Address, String, String, String, U256, bool)> {
        let chain = self.client.chain_client()?;
        chain.collections.get_collection(collection_id).await
    }

    /// Get all content IDs in a collection from on-chain.
    pub async fn get_collection_items(
        &self,
        collection_id: U256,
    ) -> Result<Vec<FixedBytes<32>>> {
        let chain = self.client.chain_client()?;
        chain.collections.get_collection_items(collection_id).await
    }

    /// Get all collection IDs for a creator from on-chain.
    pub async fn get_creator_collections(&self, creator: Address) -> Result<Vec<U256>> {
        let chain = self.client.chain_client()?;
        chain.collections.get_creator_collections(creator).await
    }

    /// Get which collection a content item belongs to (0 = none).
    pub async fn content_collection(&self, content_id: FixedBytes<32>) -> Result<U256> {
        let chain = self.client.chain_client()?;
        chain.collections.content_collection(content_id).await
    }
}
