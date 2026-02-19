use serde::{Deserialize, Serialize};

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub ethereum: EthereumConfig,
    pub iroh: IrohConfig,
    pub storage: StorageConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EthereumConfig {
    /// Ethereum JSON-RPC endpoint
    pub rpc_url: String,
    /// Chain ID (1 = mainnet, 11155111 = Sepolia)
    pub chain_id: u64,
    /// ARA token contract address
    pub ara_token_address: String,
    /// AraStaking contract address
    pub staking_address: String,
    /// ContentRegistry contract address
    pub registry_address: String,
    /// Marketplace contract address
    pub marketplace_address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IrohConfig {
    /// iroh relay server URLs
    pub relay_urls: Vec<String>,
    /// Local iroh data directory
    pub data_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// SQLite database file path
    pub db_path: String,
    /// Directory for downloaded content
    pub downloads_dir: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            ethereum: EthereumConfig {
                // Sepolia testnet — set SEPOLIA_RPC_URL env var to override with Alchemy/Infura key
                rpc_url: "https://ethereum-sepolia.publicnode.com".to_string(),
                chain_id: 11155111,
                // Sepolia deployment (2026-02-19) — with per-creator nonce in ContentRegistry
                ara_token_address: "0xE8486e01aA1Da716448a3893792837AF9f1bBFa2".to_string(),
                staking_address: "0x119554583bDB704CdA18f674054C2C7EF4C2A60c".to_string(),
                registry_address: "0x2ECb7C21A99BcB52CD202a94484C935b31cB0Ea0".to_string(),
                marketplace_address: "0xA4bBCCBFc6F7C12ad80c45C0aed386289636Bb6E".to_string(),
            },
            iroh: IrohConfig {
                relay_urls: vec!["https://relay.iroh.network".to_string()],
                data_dir: "data/iroh".to_string(),
            },
            storage: StorageConfig {
                db_path: "data/ara-marketplace.db".to_string(),
                downloads_dir: "downloads".to_string(),
            },
        }
    }
}
