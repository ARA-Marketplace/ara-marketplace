use alloy::primitives::Address;
use alloy::providers::Provider;
use alloy::sol_types::SolCall;
use anyhow::Result;

use crate::contracts::IAraNameRegistry;

/// Wrapper for AraNameRegistry contract interactions.
pub struct NameRegistryClient<P> {
    address: Address,
    provider: P,
}

impl<P: Provider + Clone> NameRegistryClient<P> {
    pub fn new(address: Address, provider: P) -> Self {
        Self { address, provider }
    }

    pub fn address(&self) -> Address {
        self.address
    }

    /// Get the display name for an address.
    pub async fn get_name(&self, user: Address) -> Result<String> {
        let contract = IAraNameRegistry::new(self.address, &self.provider);
        let result = contract.getName(user).call().await?;
        Ok(result)
    }

    /// Batch get display names for multiple addresses.
    pub async fn get_names(&self, users: Vec<Address>) -> Result<Vec<String>> {
        let contract = IAraNameRegistry::new(self.address, &self.provider);
        let result = contract.getNames(users).call().await?;
        Ok(result)
    }

    /// Reverse lookup: find the address that owns a name.
    pub async fn get_address(&self, name: &str) -> Result<Address> {
        let contract = IAraNameRegistry::new(self.address, &self.provider);
        let result = contract.getAddress(name.to_string()).call().await?;
        Ok(result)
    }
}

// Calldata encoding — no provider needed.
impl<P> NameRegistryClient<P> {
    pub fn register_name_calldata(name: &str) -> Vec<u8> {
        IAraNameRegistry::registerNameCall {
            name: name.to_string(),
        }
        .abi_encode()
    }

    pub fn remove_name_calldata() -> Vec<u8> {
        IAraNameRegistry::removeNameCall {}.abi_encode()
    }
}
