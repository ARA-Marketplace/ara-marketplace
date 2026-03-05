use alloy::primitives::U256;
use serde::{Deserialize, Serialize};

/// An unsigned transaction to be signed and broadcast.
/// The SDK constructs calldata; the caller signs via their preferred method.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionRequest {
    /// Contract address to call (0x-prefixed hex)
    pub to: String,
    /// ABI-encoded calldata (0x-prefixed hex)
    pub data: String,
    /// ETH value to send in wei (0x-prefixed hex, "0x0" for non-payable)
    pub value: String,
    /// Human-readable description for logging/display
    pub description: String,
}

/// Result of preparing a publish transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishPrepareResult {
    /// BLAKE3 content hash (0x-prefixed hex)
    pub content_hash: String,
    /// Metadata URI (JSON string)
    pub metadata_uri: String,
    /// Transactions to sign and submit
    pub transactions: Vec<TransactionRequest>,
}

/// Result of preparing a purchase transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PurchasePrepareResult {
    pub content_id: String,
    pub title: String,
    pub price_display: String,
    pub price_unit: String,
    pub transactions: Vec<TransactionRequest>,
}

/// Content detail information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentInfo {
    pub content_id: String,
    pub content_hash: String,
    pub creator: String,
    pub title: String,
    pub description: String,
    pub content_type: String,
    pub price_wei: String,
    pub price_display: String,
    pub price_unit: String,
    pub active: bool,
    pub metadata_uri: String,
    pub categories: Vec<String>,
    pub max_supply: i64,
    pub total_minted: i64,
    pub payment_token: Option<String>,
}

/// Staking information for a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StakeInfo {
    pub total_staked_wei: String,
    pub general_balance_wei: String,
    pub eth_reward_earned_wei: String,
    pub total_user_stake_wei: String,
}

/// Token reward information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenRewardInfo {
    pub token_address: String,
    pub symbol: String,
    pub earned_raw: String,
    pub earned_display: String,
}

/// Sync progress result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    pub new_content: u32,
    pub updated: u32,
    pub delisted: u32,
    pub from_block: u64,
    pub to_block: u64,
}

// ─── Formatting helpers ─────────────────────────────────────────────────────

/// Format a U256 wei value as a decimal ETH/token string (18 decimals).
pub fn format_wei(wei: U256) -> String {
    format_token_amount(wei, 18)
}

/// Format a U256 smallest-unit value with the given decimal count.
pub fn format_token_amount(value: U256, decimals: u8) -> String {
    let d = decimals as usize;
    let s = value.to_string();
    if s.len() <= d {
        let padded = format!("{:0>width$}", s, width = d + 1);
        let (whole, frac) = padded.split_at(padded.len() - d);
        let trimmed = frac.trim_end_matches('0');
        if trimmed.is_empty() {
            format!("{whole}.0")
        } else {
            format!("{whole}.{trimmed}")
        }
    } else {
        let (whole, frac) = s.split_at(s.len() - d);
        let trimmed = frac.trim_end_matches('0');
        if trimmed.is_empty() {
            format!("{whole}.0")
        } else {
            format!("{whole}.{trimmed}")
        }
    }
}

/// Parse a decimal amount string to U256 smallest units.
pub fn parse_amount(amount: &str, decimals: u8) -> anyhow::Result<U256> {
    let d = decimals as usize;
    let parts: Vec<&str> = amount.split('.').collect();
    if parts.len() > 2 {
        anyhow::bail!("Invalid amount format");
    }
    let whole = parts[0];
    let frac = if parts.len() == 2 { parts[1] } else { "" };
    if frac.len() > d {
        anyhow::bail!("Too many decimal places (max {d})");
    }
    let padded_frac = format!("{frac:0<width$}", width = d);
    let combined = format!("{whole}{padded_frac}");
    Ok(combined.parse::<U256>()?)
}

/// Encode bytes as 0x-prefixed hex string.
pub fn hex_encode(data: &[u8]) -> String {
    format!("0x{}", alloy::hex::encode(data))
}
