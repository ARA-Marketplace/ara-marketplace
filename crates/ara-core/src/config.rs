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
                // Sepolia deployment (2026-02-27) — ERC-1155 + UUPS proxies + V2 staker rewards + collections + names
                ara_token_address: "0x53720EcdDF71fE618c7A5aEc99ac2e958ad4dF99".to_string(),
                staking_address: "0xfD41Ae37cD729b6a70e42641ea14187e213b29e6".to_string(),
                registry_address: "0xd45ff950bBC1c823F66C4EbdF72De23Eb02e4831".to_string(),
                marketplace_address: "0xD7992b6A863FBacE3BB58BFE5D31EAe580adF4E0".to_string(),
                collections_address: "0x59453f1f12D10e4B4210fae8188d666011292997".to_string(),
                name_registry_address: "0xDA5827A8659271C44174894bbA403FD264198C5d".to_string(),
                moderation_address: String::new(), // Not yet deployed
                deployment_block: 10_349_200, // Sepolia deploy block (2026-02-27)
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
