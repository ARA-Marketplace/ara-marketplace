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
                // Sepolia deployment (2026-02-26) — ERC-1155 + UUPS proxies + resale marketplace
                ara_token_address: "0x6E042035Dfe8FF36527E482401D95324afaEB98e".to_string(),
                staking_address: "0x33DE0E7d909EdbFDe5973E8208C0bf3B86E553D1".to_string(),
                registry_address: "0xB893FD211bFDd9557Bd60BE96f259966db434679".to_string(),
                marketplace_address: "0x02ce6E3c0cfD96076d2Fbaf878CCB3043D225138".to_string(),
                deployment_block: 10_342_496, // Sepolia deploy block (2026-02-26)
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
