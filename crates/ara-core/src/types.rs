use serde::{Deserialize, Serialize};

/// A 32-byte BLAKE3 content hash from iroh
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContentHash(pub [u8; 32]);

impl ContentHash {
    pub fn from_hex(hex: &str) -> anyhow::Result<Self> {
        let bytes = hex::decode(hex)?;
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid hash length"))?;
        Ok(Self(arr))
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

/// On-chain content identifier (keccak256 of contentHash + creator)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContentId(pub [u8; 32]);

impl ContentId {
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

/// Content metadata stored off-chain (IPFS/Arweave)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentMetadata {
    pub title: String,
    pub description: String,
    pub content_type: ContentType,
    pub thumbnail_url: Option<String>,
    pub file_size_bytes: u64,
    pub file_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentType {
    Game,
    Music,
    Video,
    Document,
    Software,
    Other,
}

/// Seeder metrics for a single content, used for reward calculation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeederMetrics {
    pub seeder_address: String,
    pub content_id: ContentId,
    pub bytes_served: u64,
    pub ara_staked: u64,
    pub peer_count: u32,
    pub uptime_seconds: u64,
}

/// Reward distribution record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardRecord {
    pub content_id: ContentId,
    pub seeder_address: String,
    pub amount_wei: String,
    pub timestamp: u64,
    pub tx_hash: Option<String>,
}
