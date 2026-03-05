use alloy::providers::Provider;
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::Mutex;

use ara_chain::{connect_http, ContractAddresses};
use ara_core::config::AppConfig;
use ara_core::storage::Database;

use crate::signer::Signer;

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
    pub fn wallet_address(&self) -> Option<alloy::primitives::Address> {
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

        // Ensure parent directory exists
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
