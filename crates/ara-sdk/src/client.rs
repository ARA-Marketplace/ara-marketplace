use alloy::primitives::Address;
use alloy::providers::Provider;
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::Mutex;

use ara_chain::{connect_http, ContractAddresses};
use ara_core::config::AppConfig;
use ara_core::storage::Database;

use crate::signer::Signer;
use crate::types::{format_wei, BalanceInfo, TransactionRequest};

/// Main entry point for the Ara SDK. Provides access to all marketplace operations.
///
/// Use `AraClient::builder()` to construct.
pub struct AraClient {
    pub(crate) config: AppConfig,
    pub(crate) db: Arc<Mutex<Database>>,
    pub(crate) signer: Option<Arc<dyn Signer>>,
}

impl AraClient {
    /// Create a new builder.
    pub fn builder() -> AraClientBuilder {
        AraClientBuilder::default()
    }

    /// Get a reference to the configuration.
    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    /// Get the signer's address, if a signer is configured.
    pub fn wallet_address(&self) -> Option<Address> {
        self.signer.as_ref().map(|s| s.address())
    }

    /// Create a chain client for on-chain queries.
    pub(crate) fn chain_client(&self) -> Result<ara_chain::AraChain<impl Provider + Clone>> {
        let eth = &self.config.ethereum;
        let addresses = ContractAddresses {
            ara_token: eth.ara_token_address.parse()?,
            staking: eth.staking_address.parse()?,
            registry: eth.registry_address.parse()?,
            marketplace: eth.marketplace_address.parse()?,
            collections: eth.collections_address.parse().unwrap_or_default(),
            name_registry: eth.name_registry_address.parse().unwrap_or_default(),
            moderation: eth.moderation_address.parse().unwrap_or_default(),
        };
        let chain = connect_http(&eth.rpc_url, addresses)?;
        Ok(chain)
    }

    /// Get ETH, ARA token, and staked ARA balances for an address.
    pub async fn get_balances(&self, address: Address) -> Result<BalanceInfo> {
        let chain = self.chain_client()?;
        let eth_bal = chain.get_eth_balance(address).await?;
        let ara_bal = chain.token.balance_of(address).await?;
        let staked = chain.staking.staked_balance(address).await?;

        Ok(BalanceInfo {
            eth_wei: eth_bal.to_string(),
            eth_display: format_wei(eth_bal),
            ara_wei: ara_bal.to_string(),
            ara_display: format_wei(ara_bal),
            staked_wei: staked.to_string(),
            staked_display: format_wei(staked),
        })
    }

    /// Wait for a transaction to be confirmed. Polls every 3 seconds, times out after 5 minutes.
    pub async fn wait_for_transaction(&self, tx_hash: &str) -> Result<()> {
        let chain = self.chain_client()?;
        let hash_hex = tx_hash.strip_prefix("0x").unwrap_or(tx_hash);
        let hash_bytes = alloy::hex::decode(hash_hex)?;
        let hash = alloy::primitives::TxHash::from_slice(&hash_bytes);

        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(300);

        loop {
            if start.elapsed() > timeout {
                anyhow::bail!("Transaction {} not confirmed after 5 minutes", tx_hash);
            }

            match chain.provider().get_transaction_receipt(hash).await? {
                Some(receipt) => {
                    if !receipt.status() {
                        anyhow::bail!("Transaction {} reverted", tx_hash);
                    }
                    return Ok(());
                }
                None => {
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                }
            }
        }
    }

    /// Sign, send, and wait for a single transaction. Requires a signer.
    pub async fn execute_transaction(&self, tx: &TransactionRequest) -> Result<String> {
        let signer = self.signer.as_ref()
            .ok_or_else(|| anyhow::anyhow!("No signer configured — cannot execute transactions"))?;
        let tx_hash = signer.sign_and_send(tx).await?;
        Ok(tx_hash)
    }

    /// Execute multiple transactions sequentially (e.g., approve + main tx).
    /// Returns all transaction hashes.
    pub async fn execute_transactions(&self, txs: &[TransactionRequest]) -> Result<Vec<String>> {
        let mut hashes = Vec::with_capacity(txs.len());
        for tx in txs {
            let hash = self.execute_transaction(tx).await?;
            hashes.push(hash);
        }
        Ok(hashes)
    }

    /// Access content operations (publish, update, delist, query).
    pub fn content(&self) -> crate::content::ContentOps<'_> {
        crate::content::ContentOps { client: self }
    }

    /// Access marketplace operations (purchase, resale, rewards).
    pub fn marketplace(&self) -> crate::marketplace::MarketplaceOps<'_> {
        crate::marketplace::MarketplaceOps { client: self }
    }

    /// Access staking operations (stake, unstake, claim rewards).
    pub fn staking(&self) -> crate::staking::StakingOps<'_> {
        crate::staking::StakingOps { client: self }
    }

    /// Access collection operations (create, update, manage items).
    pub fn collections(&self) -> crate::collections::CollectionOps<'_> {
        crate::collections::CollectionOps { client: self }
    }

    /// Access name registry operations (register, remove, lookup).
    pub fn names(&self) -> crate::names::NameOps<'_> {
        crate::names::NameOps { client: self }
    }

    /// Access moderation operations (flag, vote, resolve).
    pub fn moderation(&self) -> crate::moderation::ModerationOps<'_> {
        crate::moderation::ModerationOps { client: self }
    }

    /// Access analytics queries (price history, trending, collectors).
    pub fn analytics(&self) -> crate::analytics::AnalyticsOps<'_> {
        crate::analytics::AnalyticsOps { client: self }
    }

    /// Access sync operations (sync content, sync rewards from chain).
    pub fn sync(&self) -> crate::sync::SyncOps<'_> {
        crate::sync::SyncOps { client: self }
    }
}

/// Builder for constructing an `AraClient`.
#[derive(Default)]
pub struct AraClientBuilder {
    config: Option<AppConfig>,
    signer: Option<Arc<dyn Signer>>,
    db_path: Option<String>,
}

impl AraClientBuilder {
    /// Set the application configuration.
    pub fn config(mut self, config: AppConfig) -> Self {
        self.config = Some(config);
        self
    }

    /// Set the transaction signer. Optional — without a signer, only read operations
    /// and calldata preparation work.
    pub fn signer(mut self, signer: impl Signer + 'static) -> Self {
        self.signer = Some(Arc::new(signer));
        self
    }

    /// Override the database path (defaults to config.storage.db_path).
    pub fn db_path(mut self, path: &str) -> Self {
        self.db_path = Some(path.to_string());
        self
    }

    /// Build the `AraClient`. Opens (or creates) the SQLite database.
    pub async fn build(self) -> Result<AraClient> {
        let config = self.config.unwrap_or_default();
        let db_path = self.db_path.unwrap_or_else(|| config.storage.db_path.clone());

        if let Some(parent) = std::path::Path::new(&db_path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let db = Database::open(&db_path)?;

        Ok(AraClient {
            config,
            db: Arc::new(Mutex::new(db)),
            signer: self.signer,
        })
    }

    /// Build the `AraClient` with an in-memory database (for testing).
    pub async fn build_in_memory(self) -> Result<AraClient> {
        let config = self.config.unwrap_or_default();
        let db = Database::open_in_memory()?;

        Ok(AraClient {
            config,
            db: Arc::new(Mutex::new(db)),
            signer: self.signer,
        })
    }
}
