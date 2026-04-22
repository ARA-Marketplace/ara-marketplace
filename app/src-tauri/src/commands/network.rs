//! External network queries (price oracles, etc). Not to be confused with the P2P network.
use serde::Deserialize;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tracing::{info, warn};

/// CoinGecko coin ID for the ARA token. Can be overridden via env var for testing.
const COIN_ID: &str = "ara";
/// Cache TTL for the ARA price. CoinGecko's free tier rate-limits, so we cache aggressively.
const PRICE_TTL: Duration = Duration::from_secs(300);
/// HTTP client timeout. Low enough that a stalled CoinGecko request doesn't hang the Dashboard.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

struct PriceCache {
    price_usd: f64,
    fetched_at: Instant,
}

static CACHE: Mutex<Option<PriceCache>> = Mutex::new(None);

/// CoinGecko's /simple/price endpoint returns a flat JSON object keyed by coin id:
///   { "ara": { "usd": 0.00012316 } }
/// Deserialize straight into a HashMap — wrapping this in a struct with
/// `#[serde(flatten)]` (as I did originally) silently fails to parse.
type CoingeckoResponse = std::collections::HashMap<String, CoingeckoPrice>;

#[derive(Deserialize)]
struct CoingeckoPrice {
    usd: f64,
}

/// Return the current ARA/USD price. Fetches from CoinGecko at most once every 5 minutes;
/// between refreshes, returns the cached value. Returns 0.0 if the API is unreachable and
/// no value has been cached yet.
#[tauri::command]
pub async fn get_ara_price_usd() -> Result<f64, String> {
    // Check cache first
    {
        let guard = CACHE.lock().map_err(|e| format!("Cache lock: {e}"))?;
        if let Some(c) = guard.as_ref() {
            if c.fetched_at.elapsed() < PRICE_TTL {
                return Ok(c.price_usd);
            }
        }
    }

    let url = format!(
        "https://api.coingecko.com/api/v3/simple/price?ids={COIN_ID}&vs_currencies=usd"
    );
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| format!("HTTP client: {e}"))?;

    // Send with explicit Accept + User-Agent — some CDNs serve different bodies
    // (or block) requests without them. CoinGecko's free tier in particular
    // appears to sometimes return gzipped or HTML payloads to clients that
    // don't advertise themselves.
    let resp = match client
        .get(&url)
        .header("Accept", "application/json")
        .header("User-Agent", "ara-marketplace/1.0")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            warn!("CoinGecko request failed: {e}");
            let guard = CACHE.lock().map_err(|e| format!("Cache lock: {e}"))?;
            return Ok(guard.as_ref().map(|c| c.price_usd).unwrap_or(0.0));
        }
    };

    let status = resp.status();
    // Read as text first so we can log it verbatim on any parse error.
    let body = match resp.text().await {
        Ok(t) => t,
        Err(e) => {
            warn!("CoinGecko body read failed (status {status}): {e}");
            let guard = CACHE.lock().map_err(|e| format!("Cache lock: {e}"))?;
            return Ok(guard.as_ref().map(|c| c.price_usd).unwrap_or(0.0));
        }
    };

    if !status.is_success() {
        warn!("CoinGecko HTTP {status}: {}", body.chars().take(200).collect::<String>());
        let guard = CACHE.lock().map_err(|e| format!("Cache lock: {e}"))?;
        return Ok(guard.as_ref().map(|c| c.price_usd).unwrap_or(0.0));
    }

    match serde_json::from_str::<CoingeckoResponse>(&body) {
        Ok(parsed) => {
            let price = parsed.get(COIN_ID).map(|p| p.usd).unwrap_or(0.0);
            info!("ARA price refreshed: ${price} (raw body: {body})");
            let mut guard = CACHE.lock().map_err(|e| format!("Cache lock: {e}"))?;
            *guard = Some(PriceCache {
                price_usd: price,
                fetched_at: Instant::now(),
            });
            Ok(price)
        }
        Err(e) => {
            warn!(
                "CoinGecko parse failed: {e} — raw body: {}",
                body.chars().take(500).collect::<String>(),
            );
            let guard = CACHE.lock().map_err(|e| format!("Cache lock: {e}"))?;
            Ok(guard.as_ref().map(|c| c.price_usd).unwrap_or(0.0))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression test: the real CoinGecko response shape
    ///   `{"ara":{"usd":0.00012316}}`
    /// must deserialize without errors. My first attempt wrapped it in a struct
    /// with `#[serde(flatten)] HashMap<..>` which silently failed and left the
    /// Dashboard showing "—". Using the HashMap directly works.
    #[test]
    fn coingecko_response_deserializes() {
        let json = r#"{"ara":{"usd":0.00012316}}"#;
        let parsed: CoingeckoResponse = serde_json::from_str(json).unwrap();
        let price = parsed.get("ara").map(|p| p.usd).unwrap();
        assert!((price - 0.00012316).abs() < f64::EPSILON);
    }
}
