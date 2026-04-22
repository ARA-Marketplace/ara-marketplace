use alloy::primitives::U256;
use serde::Serialize;
use tauri::State;

use crate::state::AppState;
use super::types::{format_token_amount, format_wei};

#[derive(Debug, Serialize)]
pub struct PricePoint {
    pub price_eth: String,
    pub block_number: i64,
    pub buyer: String,
    pub tx_hash: String,
    pub is_resale: bool,
}

#[derive(Debug, Serialize)]
pub struct ItemAnalytics {
    pub total_sales: u32,
    pub total_volume_eth: String,
    pub unique_buyers: u32,
}

#[derive(Debug, Serialize)]
pub struct CollectorRanking {
    pub address: String,
    pub purchase_count: u32,
    pub total_spent_eth: String,
}

#[derive(Debug, Serialize)]
pub struct TrendingItem {
    pub content_id: String,
    pub recent_sales: u32,
    pub title: String,
    pub price_eth: String,
    pub content_type: String,
}

#[derive(Debug, Serialize)]
pub struct TokenVolume {
    /// Display symbol ("ETH", "USDC", etc.). Unknown ERC-20s fall back to a short address.
    pub symbol: String,
    /// ERC-20 contract address (empty for native ETH).
    pub address: String,
    pub decimals: u8,
    /// Formatted decimal string — ready to render (e.g. "0.123", "0.000250").
    pub amount: String,
    /// Raw smallest-unit total as a decimal string (wei for ETH, base units for tokens).
    pub raw: String,
}

#[derive(Debug, Serialize)]
pub struct MarketplaceOverview {
    /// Total ETH volume. Retained as a top-level field for back-compat with existing UI.
    pub total_volume_eth: String,
    /// Volume grouped by payment token — one entry per currency with non-zero volume.
    /// Includes ETH as its own entry; the frontend can render a card per token.
    pub volume_by_token: Vec<TokenVolume>,
    pub total_sales: u32,
    pub total_collections: u32,
    pub total_items: u32,
    /// Total ARA staked network-wide (formatted with 18 decimals). Queried on-chain.
    pub total_staked_ara: String,
    /// Total ETH paid out as seeder rewards (lifetime). Queried on-chain.
    pub total_rewards_paid_eth: String,
}

#[tauri::command]
pub async fn get_price_history(
    state: State<'_, AppState>,
    content_id: String,
) -> Result<Vec<PricePoint>, String> {
    let db = state.db.lock().await;
    let rows = db.get_price_history(&content_id)
        .map_err(|e| format!("DB error: {e}"))?;

    Ok(rows.into_iter().map(|(price_wei, block, buyer, tx_hash, is_resale)| {
        let wei: U256 = price_wei.parse().unwrap_or(U256::ZERO);
        PricePoint {
            price_eth: format_wei(wei),
            block_number: block,
            buyer,
            tx_hash,
            is_resale,
        }
    }).collect())
}

#[tauri::command]
pub async fn get_item_analytics(
    state: State<'_, AppState>,
    content_id: String,
) -> Result<ItemAnalytics, String> {
    let db = state.db.lock().await;
    let conn = db.conn();

    let total_sales: u32 = conn.query_row(
        "SELECT COUNT(*) FROM all_purchases WHERE content_id = ?1",
        rusqlite::params![&content_id],
        |row| row.get(0),
    ).unwrap_or(0);

    let total_volume: String = conn.query_row(
        "SELECT COALESCE(SUM(CAST(price_paid_wei AS INTEGER)), 0) FROM all_purchases WHERE content_id = ?1",
        rusqlite::params![&content_id],
        |row| row.get::<_, String>(0),
    ).unwrap_or_else(|_| "0".to_string());

    let unique_buyers: u32 = conn.query_row(
        "SELECT COUNT(DISTINCT buyer) FROM all_purchases WHERE content_id = ?1",
        rusqlite::params![&content_id],
        |row| row.get(0),
    ).unwrap_or(0);

    let vol_wei: U256 = total_volume.parse().unwrap_or(U256::ZERO);

    Ok(ItemAnalytics {
        total_sales,
        total_volume_eth: format_wei(vol_wei),
        unique_buyers,
    })
}

#[tauri::command]
pub async fn get_top_collectors(
    state: State<'_, AppState>,
    limit: Option<u32>,
) -> Result<Vec<CollectorRanking>, String> {
    let db = state.db.lock().await;
    let rows = db.get_top_collectors(limit.unwrap_or(10))
        .map_err(|e| format!("DB error: {e}"))?;

    Ok(rows.into_iter().map(|(address, count, total_wei)| {
        let wei: U256 = total_wei.parse().unwrap_or(U256::ZERO);
        CollectorRanking {
            address,
            purchase_count: count,
            total_spent_eth: format_wei(wei),
        }
    }).collect())
}

#[tauri::command]
pub async fn get_trending_content(
    state: State<'_, AppState>,
    limit: Option<u32>,
) -> Result<Vec<TrendingItem>, String> {
    let db = state.db.lock().await;
    // Use last ~7200 blocks (~24h on Ethereum) as the "trending" window
    let current_block: i64 = db.get_config("last_synced_block")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let since_block = (current_block - 7200).max(0);

    let rows = db.get_trending_content(limit.unwrap_or(8), since_block)
        .map_err(|e| format!("DB error: {e}"))?;

    let conn = db.conn();
    Ok(rows.into_iter().map(|(content_id, count)| {
        let (title, price_wei, content_type): (String, String, String) = conn.query_row(
            "SELECT COALESCE(title,''), COALESCE(price_wei,'0'), COALESCE(content_type,'other') FROM content WHERE content_id = ?1",
            rusqlite::params![&content_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).unwrap_or_else(|_| ("Unknown".to_string(), "0".to_string(), "other".to_string()));

        let wei: U256 = price_wei.parse().unwrap_or(U256::ZERO);
        TrendingItem {
            content_id,
            recent_sales: count,
            title,
            price_eth: format_wei(wei),
            content_type,
        }
    }).collect())
}

// ─── Collection-level analytics ──────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CollectionAnalytics {
    pub total_volume_eth: String,
    pub total_sales: u32,
    pub unique_owners: u32,
    pub floor_price_eth: String,
    pub total_minted: u32,
}

#[derive(Debug, Serialize)]
pub struct CollectionActivity {
    pub content_id: String,
    pub title: String,
    pub buyer: String,
    pub price_eth: String,
    pub tx_hash: String,
    pub block_number: i64,
    pub is_resale: bool,
}

#[tauri::command]
pub async fn get_collection_analytics(
    state: State<'_, AppState>,
    collection_id: i64,
) -> Result<CollectionAnalytics, String> {
    let db = state.db.lock().await;
    let conn = db.conn();

    let total_sales: u32 = conn.query_row(
        "SELECT COUNT(*) FROM all_purchases ap
         JOIN collection_items ci ON ci.content_id = ap.content_id
         WHERE ci.collection_id = ?1",
        rusqlite::params![collection_id],
        |row| row.get(0),
    ).unwrap_or(0);

    let total_volume: String = conn.query_row(
        "SELECT COALESCE(SUM(CAST(ap.price_paid_wei AS INTEGER)), 0) FROM all_purchases ap
         JOIN collection_items ci ON ci.content_id = ap.content_id
         WHERE ci.collection_id = ?1",
        rusqlite::params![collection_id],
        |row| row.get::<_, String>(0),
    ).unwrap_or_else(|_| "0".to_string());

    let unique_owners: u32 = conn.query_row(
        "SELECT COUNT(DISTINCT ap.buyer) FROM all_purchases ap
         JOIN collection_items ci ON ci.content_id = ap.content_id
         WHERE ci.collection_id = ?1",
        rusqlite::params![collection_id],
        |row| row.get(0),
    ).unwrap_or(0);

    let floor_price: String = conn.query_row(
        "SELECT COALESCE(MIN(CAST(ct.price_wei AS INTEGER)), 0) FROM collection_items ci
         JOIN content ct ON ct.content_id = ci.content_id AND ct.active = 1
         WHERE ci.collection_id = ?1",
        rusqlite::params![collection_id],
        |row| row.get::<_, String>(0),
    ).unwrap_or_else(|_| "0".to_string());

    let total_minted: u32 = conn.query_row(
        "SELECT COALESCE(SUM(ct.total_minted), 0) FROM collection_items ci
         JOIN content ct ON ct.content_id = ci.content_id
         WHERE ci.collection_id = ?1",
        rusqlite::params![collection_id],
        |row| row.get(0),
    ).unwrap_or(0);

    let vol_wei: U256 = total_volume.parse().unwrap_or(U256::ZERO);
    let floor_wei: U256 = floor_price.parse().unwrap_or(U256::ZERO);

    Ok(CollectionAnalytics {
        total_volume_eth: format_wei(vol_wei),
        total_sales,
        unique_owners,
        floor_price_eth: format_wei(floor_wei),
        total_minted,
    })
}

#[tauri::command]
pub async fn get_collection_activity(
    state: State<'_, AppState>,
    collection_id: i64,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<CollectionActivity>, String> {
    let db = state.db.lock().await;
    let conn = db.conn();
    let lim = limit.unwrap_or(50) as i64;
    let off = offset.unwrap_or(0) as i64;

    let mut stmt = conn.prepare(
        "SELECT ap.content_id, COALESCE(ct.title, 'Unknown'), ap.buyer,
                ap.price_paid_wei, ap.tx_hash, ap.block_number, ap.is_resale
         FROM all_purchases ap
         JOIN collection_items ci ON ci.content_id = ap.content_id
         LEFT JOIN content ct ON ct.content_id = ap.content_id
         WHERE ci.collection_id = ?1
         ORDER BY ap.block_number DESC
         LIMIT ?2 OFFSET ?3"
    ).map_err(|e| format!("DB error: {e}"))?;

    let rows = stmt.query_map(
        rusqlite::params![collection_id, lim, off],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, bool>(6)?,
            ))
        },
    ).map_err(|e| format!("DB error: {e}"))?;

    let mut result = Vec::new();
    for row in rows {
        let (content_id, title, buyer, price_wei, tx_hash, block_number, is_resale) =
            row.map_err(|e| format!("Row error: {e}"))?;
        let wei: U256 = price_wei.parse().unwrap_or(U256::ZERO);
        result.push(CollectionActivity {
            content_id,
            title,
            buyer,
            price_eth: format_wei(wei),
            tx_hash,
            block_number,
            is_resale,
        });
    }
    Ok(result)
}

// ─── Marketplace overview ────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_marketplace_overview(
    state: State<'_, AppState>,
) -> Result<MarketplaceOverview, String> {
    let db = state.db.lock().await;
    let conn = db.conn();

    let total_sales: u32 = conn.query_row(
        "SELECT COUNT(*) FROM all_purchases",
        [],
        |row| row.get(0),
    ).unwrap_or(0);

    // Per-token volume: SELECT raw wei + payment_token per purchase and sum in Rust with U256.
    //
    // SQLite's INTEGER is i64 (max ~9.22e18), so `SUM(CAST price_wei AS INTEGER)` silently
    // overflows once cumulative per-token volume exceeds ~9.22 ETH. Pulling raw TEXT wei
    // values and summing with U256 avoids that — wei values are already stored as TEXT, so
    // no casting round-trip is required. Both `total_volume_eth` (back-compat) and the
    // per-token breakdown come from this single pass.
    let per_purchase_rows: Vec<(String, String)> = (|| -> Result<Vec<(String, String)>, String> {
        let mut stmt = conn
            .prepare(
                "SELECT COALESCE(LOWER(c.payment_token), '') AS token, p.price_paid_wei
                 FROM all_purchases p
                 LEFT JOIN content c ON p.content_id = c.content_id",
            )
            .map_err(|e| format!("per-purchase prepare failed: {e}"))?;
        let iter = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .map_err(|e| format!("per-purchase query_map failed: {e}"))?;
        let mut out = Vec::new();
        for r in iter {
            match r {
                Ok(row) => out.push(row),
                Err(e) => tracing::warn!("per-purchase row error: {e}"),
            }
        }
        Ok(out)
    })()
    .unwrap_or_else(|e| {
        tracing::warn!("{e}");
        Vec::new()
    });

    // Aggregate into a map: token_address (lowercase; "" for native ETH) → total U256
    let mut totals_by_token: std::collections::HashMap<String, U256> =
        std::collections::HashMap::new();
    for (token_addr, wei_str) in &per_purchase_rows {
        let wei: U256 = wei_str.parse().unwrap_or(U256::ZERO);
        *totals_by_token.entry(token_addr.clone()).or_insert(U256::ZERO) += wei;
    }

    // Derive the legacy ETH-only total for back-compat with existing UI.
    let eth_total: U256 = totals_by_token
        .iter()
        .filter(|(addr, _)| {
            addr.is_empty() || addr.as_str() == "0x0000000000000000000000000000000000000000"
        })
        .map(|(_, v)| *v)
        .fold(U256::ZERO, |acc, v| acc + v);

    let total_collections: u32 = conn.query_row(
        "SELECT COUNT(*) FROM collections WHERE active = 1",
        [],
        |row| row.get(0),
    ).unwrap_or(0);

    let total_items: u32 = conn.query_row(
        "SELECT COUNT(*) FROM content WHERE active = 1",
        [],
        |row| row.get(0),
    ).unwrap_or(0);

    let vol_wei: U256 = eth_total;
    drop(db);

    // Resolve aggregated per-token totals into user-facing TokenVolume entries using the
    // configured `supported_tokens` list for symbol + decimals.
    let mut volume_by_token: Vec<TokenVolume> = totals_by_token
        .into_iter()
        .filter_map(|(token_addr, raw_u)| {
            if raw_u.is_zero() {
                return None;
            }
            let is_native = token_addr.is_empty()
                || token_addr == "0x0000000000000000000000000000000000000000";
            if is_native {
                Some(TokenVolume {
                    symbol: "ETH".to_string(),
                    address: String::new(),
                    decimals: 18,
                    amount: format_token_amount(raw_u, 18),
                    raw: raw_u.to_string(),
                })
            } else {
                let cfg = state
                    .config
                    .ethereum
                    .supported_tokens
                    .iter()
                    .find(|t| t.address.to_lowercase() == token_addr);
                let (symbol, decimals) = match cfg {
                    Some(t) => (t.symbol.clone(), t.decimals),
                    None => {
                        let short = if token_addr.len() > 10 {
                            format!("{}…{}", &token_addr[..6], &token_addr[token_addr.len() - 4..])
                        } else {
                            token_addr.clone()
                        };
                        (short, 18)
                    }
                };
                Some(TokenVolume {
                    symbol,
                    address: token_addr,
                    decimals,
                    amount: format_token_amount(raw_u, decimals),
                    raw: raw_u.to_string(),
                })
            }
        })
        .collect();
    // Stable order: ETH first, then by symbol — avoids HashMap-driven card reshuffle on refresh.
    volume_by_token.sort_by(|a, b| match (a.address.is_empty(), b.address.is_empty()) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.symbol.cmp(&b.symbol),
    });

    // Pull network-wide totals from chain. Log failures so we can actually debug
    // when they silently return zero (previously swallowed errors gave empty cards).
    let (total_staked_ara, total_rewards_paid_eth) = match state.chain_client() {
        Ok(chain) => {
            let staked = match chain.staking.total_staked().await {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("total_staked() chain call failed: {e}");
                    U256::ZERO
                }
            };
            let rewards = match chain.marketplace.total_rewards_claimed().await {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("total_rewards_claimed() chain call failed: {e}");
                    U256::ZERO
                }
            };
            tracing::info!(
                "marketplace overview: total_staked={staked}, total_rewards_claimed={rewards}"
            );
            (format_wei(staked), format_wei(rewards))
        }
        Err(e) => {
            tracing::warn!("chain_client unavailable for marketplace overview: {e}");
            ("0.0".to_string(), "0.0".to_string())
        }
    };

    Ok(MarketplaceOverview {
        total_volume_eth: format_wei(vol_wei),
        volume_by_token,
        total_sales,
        total_collections,
        total_items,
        total_staked_ara,
        total_rewards_paid_eth,
    })
}
