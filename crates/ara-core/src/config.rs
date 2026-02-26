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
    /// Block number where contracts were deployed (floor for event sync)
    pub deployment_block: u64,
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
                // Sepolia deployment (2026-02-25) — per-receipt claiming + fileSize
                ara_token_address: "0x40A13EF876e3dCf968b4dC372a92ADdCa95b8A3b".to_string(),
                staking_address: "0xCbb3958d9D6DaAc22518C90CA663CE027AD0D39F".to_string(),
                registry_address: "0x4db94B57425189EEC4C8674Fa5E8f4AC24105b32".to_string(),
                marketplace_address: "0x8Fe6db4d530538F1f419a8FD39D3C09eE18F1Cc7".to_string(),
                deployment_block: 10_337_150, // Sepolia deploy block (2026-02-25)
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
