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

#[derive(Deserialize)]
struct CoingeckoResponse {
    #[serde(flatten)]
    entries: std::collections::HashMap<String, CoingeckoPrice>,
}

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

    match client.get(&url).send().await {
        Ok(resp) => {
            let body = resp.json::<CoingeckoResponse>().await;
            match body {
                Ok(parsed) => {
                    let price = parsed
                        .entries
                        .get(COIN_ID)
                        .map(|p| p.usd)
                        .unwrap_or(0.0);
                    info!("ARA price refreshed: ${price}");
                    let mut guard = CACHE.lock().map_err(|e| format!("Cache lock: {e}"))?;
                    *guard = Some(PriceCache {
                        price_usd: price,
                        fetched_at: Instant::now(),
                    });
                    Ok(price)
                }
                Err(e) => {
                    warn!("CoinGecko parse failed: {e}");
                    // Stale cache if present, else 0
                    let guard = CACHE.lock().map_err(|e| format!("Cache lock: {e}"))?;
                    Ok(guard.as_ref().map(|c| c.price_usd).unwrap_or(0.0))
                }
            }
        }
        Err(e) => {
            warn!("CoinGecko request failed: {e}");
            let guard = CACHE.lock().map_err(|e| format!("Cache lock: {e}"))?;
            Ok(guard.as_ref().map(|c| c.price_usd).unwrap_or(0.0))
        }
    }
}
