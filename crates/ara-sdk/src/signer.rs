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
    private_key: String,
    rpc_url: String,
}

impl PrivateKeySigner {
    /// Create a new signer from a hex-encoded private key and RPC URL.
    ///
    /// The private key can be with or without a `0x` prefix.
    /// The RPC URL is used to broadcast signed transactions.
    pub fn new(private_key: &str, rpc_url: &str) -> Self {
        let key_hex = private_key.strip_prefix("0x").unwrap_or(private_key);
        let key_bytes = alloy::hex::decode(key_hex).unwrap_or_default();

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
            private_key: private_key.to_string(),
            rpc_url: rpc_url.to_string(),
        }
    }

    fn make_provider(&self) -> Result<impl alloy::providers::Provider + Clone> {
        use alloy::network::EthereumWallet;
        use alloy::providers::ProviderBuilder;
        use alloy::signers::local::PrivateKeySigner as AlloySigner;

        let key_hex = self.private_key.strip_prefix("0x").unwrap_or(&self.private_key);
        let key_bytes = alloy::hex::decode(key_hex)?;
        let signer = AlloySigner::from_slice(&key_bytes)?;
        let wallet = EthereumWallet::from(signer);

        let provider = ProviderBuilder::new()
            .wallet(wallet)
            .connect_http(self.rpc_url.parse()?);

        Ok(provider)
    }
}

#[async_trait::async_trait]
impl Signer for PrivateKeySigner {
    async fn sign_and_send(&self, tx: &TransactionRequest) -> Result<String> {
        use alloy::providers::Provider;
        use alloy::rpc::types::TransactionRequest as AlloyTxRequest;

        let provider = self.make_provider()?;

        let to_addr: Address = tx.to.parse()?;
        let data_hex = tx.data.strip_prefix("0x").unwrap_or(&tx.data);
        let data_bytes = alloy::hex::decode(data_hex)?;

        let value = if tx.value == "0x0" || tx.value.is_empty() {
            alloy::primitives::U256::ZERO
        } else {
            let v_hex = tx.value.strip_prefix("0x").unwrap_or(&tx.value);
            alloy::primitives::U256::from_str_radix(v_hex, 16)?
        };

        let alloy_tx = AlloyTxRequest::default()
            .to(to_addr)
            .input(data_bytes.into())
            .value(value);

        let pending = provider.send_transaction(alloy_tx).await?;
        let tx_hash = *pending.tx_hash();
        let receipt = pending.get_receipt().await?;

        if !receipt.status() {
            anyhow::bail!("Transaction {tx_hash:#x} reverted");
        }

        Ok(format!("{tx_hash:#x}"))
    }

    async fn sign_typed_data(&self, _domain: &str, _types: &str, _value: &str) -> Result<Vec<u8>> {
        anyhow::bail!("PrivateKeySigner.sign_typed_data not yet implemented")
    }

    fn address(&self) -> Address {
        self.address
    }
}
