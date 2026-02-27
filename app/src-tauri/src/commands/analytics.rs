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

    Ok(MarketplaceOverview {
        total_volume_eth: format_wei(vol_wei),
        total_sales,
        total_collections,
        total_items,
    })
}
