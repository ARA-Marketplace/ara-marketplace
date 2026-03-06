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

/// A collaborator for revenue splitting on content publishing.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CollaboratorInput {
    /// Ethereum wallet address (0x-prefixed hex)
    pub wallet: String,
    /// Share in basis points (sum of all collaborators must equal 10000)
    pub share_bps: u32,
}

/// Get the current Unix timestamp in seconds as i64.
/// Uses `unwrap_or_default()` to handle clock errors gracefully.
pub fn now_unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Parse a 0x-prefixed hex string into a 32-byte FixedBytes.
pub fn parse_content_id(s: &str) -> Result<alloy::primitives::FixedBytes<32>, String> {
    s.strip_prefix("0x")
        .unwrap_or(s)
        .parse()
        .map_err(|e| format!("Invalid content ID: {e}"))
}

/// Format a U256 wei value as a decimal ETH/token string (18 decimals).
/// Very small values (< 0.000001) are shown as "<0.000001" to avoid
/// extremely long decimal strings in the UI.
pub fn format_wei(wei: U256) -> String {
    format_token_amount_capped(wei, 18, 6)
}

/// Encode bytes as 0x-prefixed hex string.
pub fn hex_encode(data: &[u8]) -> String {
    format!("0x{}", alloy::hex::encode(data))
}

/// Parse a decimal token amount string (e.g. "100.5") to U256 wei (18 decimals).
pub fn parse_token_amount(amount: &str) -> Result<U256, String> {
    parse_token_amount_with_decimals(amount, 18)
}

/// Parse a decimal token amount string to its smallest unit given the token's decimal count.
/// E.g. "100.5" with decimals=6 → 100_500_000.
pub fn parse_token_amount_with_decimals(amount: &str, decimals: u8) -> Result<U256, String> {
    let d = decimals as usize;
    let parts: Vec<&str> = amount.split('.').collect();
    if parts.len() > 2 {
        return Err("Invalid amount format".to_string());
    }

    let whole = parts[0];
    let frac = if parts.len() == 2 { parts[1] } else { "" };

    if frac.len() > d {
        return Err(format!("Too many decimal places (max {d})"));
    }

    let padded_frac = format!("{frac:0<width$}", width = d);
    let combined = format!("{whole}{padded_frac}");

    combined
        .parse::<U256>()
        .map_err(|e| format!("Invalid amount: {e}"))
}

/// Format a U256 smallest-unit value as a human-readable decimal string with given decimals.
/// E.g. 100_500_000 with decimals=6 → "100.5".
pub fn format_token_amount(value: U256, decimals: u8) -> String {
    // For tokens with <= 8 decimals (USDC, etc.), show all significant digits.
    // For ETH (18 decimals), cap at 6 fractional digits.
    let max_frac = if decimals <= 8 { decimals as usize } else { 6 };
    format_token_amount_capped(value, decimals, max_frac)
}

/// Format a U256 smallest-unit value with at most `max_frac_digits` after the decimal point.
/// Values smaller than the displayable threshold show as "<0.0...01".
fn format_token_amount_capped(value: U256, decimals: u8, max_frac_digits: usize) -> String {
    if value.is_zero() {
        return "0.0".to_string();
    }

    let d = decimals as usize;
    let s = value.to_string();

    // Split into whole and fractional parts
    let (whole, frac_full) = if s.len() <= d {
        let padded = format!("{:0>width$}", s, width = d + 1);
        let split = padded.len() - d;
        let w = padded[..split].to_string();
        let f = padded[split..].to_string();
        (w, f)
    } else {
        let split = s.len() - d;
        (s[..split].to_string(), s[split..].to_string())
    };

    let trimmed = frac_full.trim_end_matches('0');

    if trimmed.is_empty() {
        return format!("{whole}.0");
    }

    // If the significant digits fit within max_frac_digits, show them
    if trimmed.len() <= max_frac_digits {
        return format!("{whole}.{trimmed}");
    }

    // If whole part is non-zero, just truncate the fraction
    if whole != "0" {
        let truncated = &frac_full[..max_frac_digits];
        let truncated = truncated.trim_end_matches('0');
        if truncated.is_empty() {
            return format!("{whole}.0");
        }
        return format!("{whole}.{truncated}");
    }

    // whole == "0" and trimmed is longer than max_frac_digits:
    // the value is very small, e.g. 0.000000000001
    // Show as "<0.000001" (with max_frac_digits zeros + 1)
    let threshold = format!("0.{:0>width$}1", "", width = max_frac_digits);
    format!("<{threshold}")
}
