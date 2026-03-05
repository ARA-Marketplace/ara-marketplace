use alloy::primitives::Address;
use anyhow::Result;

use crate::types::TransactionRequest;

/// Trait for signing Ethereum transactions. Implement this for custom wallet integrations
/// (hardware wallets, remote signers, etc.).
#[async_trait::async_trait]
pub trait Signer: Send + Sync {
    /// Sign and broadcast a transaction, returning the transaction hash (0x-prefixed hex).
    async fn sign_and_send(&self, tx: &TransactionRequest) -> Result<String>;

    /// Sign EIP-712 typed data, returning the signature bytes (65 bytes).
    async fn sign_typed_data(&self, domain_json: &str, types_json: &str, value_json: &str) -> Result<Vec<u8>>;

    /// Get the signer's Ethereum address.
    fn address(&self) -> Address;
}

/// A signer backed by a raw private key. Uses alloy for local signing and RPC submission.
pub struct PrivateKeySigner {
    address: Address,
    _private_key: String,
}

impl PrivateKeySigner {
    pub fn new(private_key: &str) -> Self {
        // Parse the private key to derive the address
        let key_bytes = private_key.strip_prefix("0x").unwrap_or(private_key);
        let key_bytes = alloy::hex::decode(key_bytes).unwrap_or_default();

        // Use alloy's signing key to derive address
        let address = if key_bytes.len() == 32 {
            use alloy::signers::local::PrivateKeySigner as AlloySigner;
            let signer: AlloySigner = AlloySigner::from_slice(&key_bytes)
                .expect("Invalid private key");
            signer.address()
        } else {
            Address::ZERO
        };

        Self {
            address,
            _private_key: private_key.to_string(),
        }
    }
}

#[async_trait::async_trait]
impl Signer for PrivateKeySigner {
    async fn sign_and_send(&self, _tx: &TransactionRequest) -> Result<String> {
        // In a full implementation, this would:
        // 1. Parse the tx fields into an alloy TransactionRequest
        // 2. Sign with the private key
        // 3. Send via the configured RPC endpoint
        // 4. Return the tx hash
        //
        // For now, return the calldata — callers use prepare-only mode
        // and submit via their own infrastructure.
        anyhow::bail!("PrivateKeySigner.sign_and_send not yet implemented — use prepare-only mode and submit transactions via your own RPC endpoint")
    }

    async fn sign_typed_data(&self, _domain: &str, _types: &str, _value: &str) -> Result<Vec<u8>> {
        anyhow::bail!("PrivateKeySigner.sign_typed_data not yet implemented")
    }

    fn address(&self) -> Address {
        self.address
    }
}
