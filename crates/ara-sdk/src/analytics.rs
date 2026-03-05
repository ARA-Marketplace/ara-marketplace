use alloy::primitives::U256;
use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::client::AraClient;
use crate::types::format_wei;

/// Analytics query types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricePoint {
    pub price_eth: String,
    pub block_number: i64,
    pub buyer: String,
    pub tx_hash: String,
    pub is_resale: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemAnalytics {
    pub total_sales: u32,
    pub total_volume_eth: String,
    pub unique_buyers: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectorRanking {
    pub address: String,
    pub purchase_count: u32,
    pub total_spent_eth: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendingItem {
    pub content_id: String,
    pub recent_sales: u32,
    pub title: String,
    pub price_eth: String,
    pub content_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceOverview {
    pub total_volume_eth: String,
    pub total_sales: u32,
    pub total_collections: u32,
    pub total_items: u32,
}

/// Analytics operations: price history, item stats, trending, collectors.
pub struct AnalyticsOps<'a> {
    pub(crate) client: &'a AraClient,
}

impl AnalyticsOps<'_> {
    /// Get price history for a content item from the local database.
    pub async fn get_price_history(&self, content_id: &str) -> Result<Vec<PricePoint>> {
        let db = self.client.db.lock().await;
        let rows = db.get_price_history(content_id)?;

        Ok(rows
            .into_iter()
            .map(|(price_wei, block, buyer, tx_hash, is_resale)| {
                let wei: U256 = price_wei.parse().unwrap_or(U256::ZERO);
                PricePoint {
                    price_eth: format_wei(wei),
                    block_number: block,
                    buyer,
                    tx_hash,
                    is_resale,
                }
            })
            .collect())
    }

    /// Get analytics summary for a content item.
    pub async fn get_item_analytics(&self, content_id: &str) -> Result<ItemAnalytics> {
        let db = self.client.db.lock().await;
        let conn = db.conn();

        let total_sales: u32 = conn
            .query_row(
                "SELECT COUNT(*) FROM all_purchases WHERE content_id = ?1",
                rusqlite::params![content_id],
                |row| row.get(0),
            )
            .unwrap_or(0);

        let total_volume: String = conn
            .query_row(
                "SELECT COALESCE(SUM(CAST(price_paid_wei AS INTEGER)), 0) FROM all_purchases WHERE content_id = ?1",
                rusqlite::params![content_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap_or_else(|_| "0".to_string());

        let unique_buyers: u32 = conn
            .query_row(
                "SELECT COUNT(DISTINCT buyer) FROM all_purchases WHERE content_id = ?1",
                rusqlite::params![content_id],
                |row| row.get(0),
            )
            .unwrap_or(0);

        let vol_wei: U256 = total_volume.parse().unwrap_or(U256::ZERO);

        Ok(ItemAnalytics {
            total_sales,
            total_volume_eth: format_wei(vol_wei),
            unique_buyers,
        })
    }

    /// Get top collectors ranked by spend.
    pub async fn get_top_collectors(&self, limit: u32) -> Result<Vec<CollectorRanking>> {
        let db = self.client.db.lock().await;
        let rows = db.get_top_collectors(limit)?;

        Ok(rows
            .into_iter()
            .map(|(address, count, total_wei)| {
                let wei: U256 = total_wei.parse().unwrap_or(U256::ZERO);
                CollectorRanking {
                    address,
                    purchase_count: count,
                    total_spent_eth: format_wei(wei),
                }
            })
            .collect())
    }

    /// Get trending content (most sales in recent blocks).
    pub async fn get_trending(&self, limit: u32) -> Result<Vec<TrendingItem>> {
        let db = self.client.db.lock().await;
        let current_block: i64 = db
            .get_config("last_synced_block")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let since_block = (current_block - 7200).max(0);

        let rows = db.get_trending_content(limit, since_block)?;
        let conn = db.conn();

        Ok(rows
            .into_iter()
            .map(|(content_id, count)| {
                let (title, price_wei, content_type): (String, String, String) = conn
                    .query_row(
                        "SELECT COALESCE(title,''), COALESCE(price_wei,'0'), COALESCE(content_type,'other') FROM content WHERE content_id = ?1",
                        rusqlite::params![&content_id],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )
                    .unwrap_or_else(|_| ("Unknown".to_string(), "0".to_string(), "other".to_string()));

                let wei: U256 = price_wei.parse().unwrap_or(U256::ZERO);
                TrendingItem {
                    content_id,
                    recent_sales: count,
                    title,
                    price_eth: format_wei(wei),
                    content_type,
                }
            })
            .collect())
    }

    /// Get marketplace-wide overview stats.
    pub async fn get_overview(&self) -> Result<MarketplaceOverview> {
        let db = self.client.db.lock().await;
        let conn = db.conn();

        let total_sales: u32 = conn
            .query_row("SELECT COUNT(*) FROM all_purchases", [], |row| row.get(0))
            .unwrap_or(0);

        let total_volume: String = conn
            .query_row(
                "SELECT COALESCE(SUM(CAST(price_paid_wei AS INTEGER)), 0) FROM all_purchases",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap_or_else(|_| "0".to_string());

        let total_collections: u32 = conn
            .query_row(
                "SELECT COUNT(*) FROM collections WHERE active = 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        let total_items: u32 = conn
            .query_row(
                "SELECT COUNT(*) FROM content WHERE active = 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        let vol_wei: U256 = total_volume.parse().unwrap_or(U256::ZERO);

        Ok(MarketplaceOverview {
            total_volume_eth: format_wei(vol_wei),
            total_sales,
            total_collections,
            total_items,
        })
    }
}
