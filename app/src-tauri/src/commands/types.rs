use alloy::primitives::U256;
use serde::{Deserialize, Serialize};

/// An unsigned transaction to be signed and broadcast by the frontend wallet.
/// The Rust backend constructs calldata; the frontend signs via WalletConnect.
#[derive(Debug, Serialize, Deserialize)]
pub struct TransactionRequest {
    /// Contract address to call (0x-prefixed hex)
    pub to: String,
    /// ABI-encoded calldata (0x-prefixed hex)
    pub data: String,
    /// ETH value to send in wei (0x-prefixed hex, "0x0" for non-payable)
    pub value: String,
    /// Human-readable description for the signing prompt
    pub description: String,
}

/// Format a U256 wei value as a decimal ETH/token string (18 decimals).
pub fn format_wei(wei: U256) -> String {
    let s = wei.to_string();
    if s.len() <= 18 {
        let padded = format!("{:0>19}", s);
        let (whole, frac) = padded.split_at(padded.len() - 18);
        let trimmed = frac.trim_end_matches('0');
        if trimmed.is_empty() {
            format!("{whole}.0")
        } else {
            format!("{whole}.{trimmed}")
        }
    } else {
        let (whole, frac) = s.split_at(s.len() - 18);
        let trimmed = frac.trim_end_matches('0');
        if trimmed.is_empty() {
            format!("{whole}.0")
        } else {
            format!("{whole}.{trimmed}")
        }
    }
}

/// Encode bytes as 0x-prefixed hex string.
pub fn hex_encode(data: &[u8]) -> String {
    format!("0x{}", alloy::hex::encode(data))
}

/// Parse a decimal token amount string (e.g. "100.5") to U256 wei.
pub fn parse_token_amount(amount: &str) -> Result<U256, String> {
    let parts: Vec<&str> = amount.split('.').collect();
    if parts.len() > 2 {
        return Err("Invalid amount format".to_string());
    }

    let whole = parts[0];
    let frac = if parts.len() == 2 { parts[1] } else { "" };

    if frac.len() > 18 {
        return Err("Too many decimal places (max 18)".to_string());
    }

    let padded_frac = format!("{:0<18}", frac);
    let combined = format!("{whole}{padded_frac}");

    combined
        .parse::<U256>()
        .map_err(|e| format!("Invalid amount: {e}"))
}
