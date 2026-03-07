use alloy::primitives::Address;
use anyhow::Result;

use ara_chain::names::NameRegistryClient;

use crate::client::AraClient;
use crate::types::{hex_encode, TransactionRequest};

/// Name registry operations: register, remove, lookup display names.
pub struct NameOps<'a> {
    pub(crate) client: &'a AraClient,
}

impl NameOps<'_> {
    /// Prepare a register-name transaction.
    pub fn prepare_register(&self, name: &str) -> Result<Vec<TransactionRequest>> {
        let eth = &self.client.config.ethereum;
        let registry_addr: Address = eth.name_registry_address.parse()?;

        let calldata = NameRegistryClient::<()>::register_name_calldata(name);

        Ok(vec![TransactionRequest {
            to: format!("{registry_addr:#x}"),
            data: hex_encode(&calldata),
            value: "0x0".to_string(),
            description: format!("Register display name \"{}\"", name),
        }])
    }

    /// Prepare a remove-name transaction.
    pub fn prepare_remove(&self) -> Result<Vec<TransactionRequest>> {
        let eth = &self.client.config.ethereum;
        let registry_addr: Address = eth.name_registry_address.parse()?;

        let calldata = NameRegistryClient::<()>::remove_name_calldata();

        Ok(vec![TransactionRequest {
            to: format!("{registry_addr:#x}"),
            data: hex_encode(&calldata),
            value: "0x0".to_string(),
            description: "Remove display name".to_string(),
        }])
    }

    /// Look up the display name for an address from on-chain.
    pub async fn get_name(&self, user: Address) -> Result<String> {
        let chain = self.client.chain_client()?;
        chain.name_registry.get_name(user).await
    }

    /// Batch look up display names for multiple addresses.
    pub async fn get_names(&self, users: Vec<Address>) -> Result<Vec<String>> {
        let chain = self.client.chain_client()?;
        chain.name_registry.get_names(users).await
    }

    /// Reverse lookup: find the address that owns a name.
    pub async fn get_address(&self, name: &str) -> Result<Address> {
        let chain = self.client.chain_client()?;
        chain.name_registry.get_address(name).await
    }

    /// Check if a name is available (not registered by anyone).
    pub async fn check_available(&self, name: &str) -> Result<bool> {
        let addr = self.get_address(name).await?;
        Ok(addr == Address::ZERO)
    }

    /// Confirm a name registration in local DB cache.
    pub async fn confirm_register(&self, address: &str, name: &str) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let db = self.client.db.lock().await;
        db.upsert_name(address, name, now)?;
        Ok(())
    }

    /// Confirm a name removal in local DB cache.
    pub async fn confirm_remove(&self, address: &str) -> Result<()> {
        let db = self.client.db.lock().await;
        db.remove_name(address)?;
        Ok(())
    }
}
