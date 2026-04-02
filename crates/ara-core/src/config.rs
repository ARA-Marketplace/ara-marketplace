use serde::{Deserialize, Serialize};

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub ethereum: EthereumConfig,
    pub iroh: IrohConfig,
    pub storage: StorageConfig,
    #[serde(default)]
    pub arweave: ArweaveConfig,
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
    /// AraCollections contract address
    #[serde(default)]
    pub collections_address: String,
    /// AraNameRegistry contract address
    #[serde(default)]
    pub name_registry_address: String,
    /// AraModeration contract address
    #[serde(default)]
    pub moderation_address: String,
    /// Block number where contracts were deployed (floor for event sync)
    pub deployment_block: u64,
    /// Supported ERC-20 payment tokens (address → {symbol, decimals})
    #[serde(default)]
    pub supported_tokens: Vec<TokenConfig>,
}

/// Configuration for a supported ERC-20 payment token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenConfig {
    /// Contract address (checksummed)
    pub address: String,
    /// Token symbol (e.g. "USDC", "DAI")
    pub symbol: String,
    /// Number of decimals (e.g. 6 for USDC, 18 for DAI)
    pub decimals: u8,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArweaveConfig {
    /// Irys bundler node URL (e.g. "https://node2.irys.xyz")
    pub node_url: String,
    /// Arweave gateway URL for downloads (e.g. "https://arweave.net")
    pub gateway_url: String,
}

impl Default for ArweaveConfig {
    fn default() -> Self {
        Self {
            node_url: "https://node2.irys.xyz".to_string(),
            gateway_url: "https://arweave.net".to_string(),
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            ethereum: EthereumConfig {
                // Sepolia testnet — set SEPOLIA_RPC_URL env var to override with Alchemy/Infura key
                rpc_url: "https://ethereum-sepolia.publicnode.com".to_string(),
                chain_id: 11155111,
                // Sepolia deployment (2026-04-01) — fresh deploy with free content + tipping
                ara_token_address: "0xA4c42cd49774d9B0af9C2D6BB88cf53b49b95b1b".to_string(),
                staking_address: "0x16e1CA6619FF0555BAFc43dEC9595C39776A2B63".to_string(),
                registry_address: "0x8C52B0b11cF5759312555ab1C6926e6Ce57297a0".to_string(),
                marketplace_address: "0xa133F5eb0aE369D627B13F0e283ACDC763Fb48c4".to_string(),
                collections_address: "0x606658d5935E788CccCDF9188308434130a7C671".to_string(),
                name_registry_address: "0x5C451d9B613468D4212AE31b5F139E759dD992FA".to_string(),
                moderation_address: String::new(), // Not yet deployed
                deployment_block: 10_569_600, // Sepolia deploy block (2026-04-01)
                supported_tokens: vec![],     // No ERC-20 tokens configured by default
            },
            iroh: IrohConfig {
                relay_urls: vec!["https://relay.iroh.network".to_string()],
                data_dir: "data/iroh".to_string(),
            },
            storage: StorageConfig {
                db_path: "data/ara-marketplace.db".to_string(),
                downloads_dir: "downloads".to_string(),
            },
            arweave: ArweaveConfig::default(),
        }
    }
}
