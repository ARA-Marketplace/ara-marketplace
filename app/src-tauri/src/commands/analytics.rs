use alloy::primitives::U256;
use serde::Serialize;
use tauri::State;

use crate::state::AppState;
use super::types::format_wei;

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
pub struct MarketplaceOverview {
    pub total_volume_eth: String,
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

    let total_volume: String = conn.query_row(
        "SELECT COALESCE(SUM(CAST(price_paid_wei AS INTEGER)), 0) FROM all_purchases",
        [],
        |row| row.get::<_, String>(0),
    ).unwrap_or_else(|_| "0".to_string());

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

    let vol_wei: U256 = total_volume.parse().unwrap_or(U256::ZERO);
    drop(db);

    // Pull network-wide totals from chain. Failures degrade gracefully to "0" — the
    // dashboard card never fails because the chain RPC is flaky.
    let (total_staked_ara, total_rewards_paid_eth) = if let Ok(chain) = state.chain_client() {
        let staked = chain
            .staking
            .total_staked()
            .await
            .unwrap_or(U256::ZERO);
        let rewards = chain
            .marketplace
            .total_rewards_claimed()
            .await
            .unwrap_or(U256::ZERO);
        (format_wei(staked), format_wei(rewards))
    } else {
        ("0.0".to_string(), "0.0".to_string())
    };

    Ok(MarketplaceOverview {
        total_volume_eth: format_wei(vol_wei),
        total_sales,
        total_collections,
        total_items,
        total_staked_ara,
        total_rewards_paid_eth,
    })
}
