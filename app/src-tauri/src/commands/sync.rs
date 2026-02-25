use crate::state::AppState;
use alloy::primitives::Address;
use ara_chain::{AraEvent, IndexedEvent};
use serde::{Deserialize, Serialize};
use tauri::State;
use tracing::{info, warn};

/// Metadata embedded in the on-chain metadataURI (v1/v2 JSON format).
#[derive(Deserialize)]
struct MetadataV1 {
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    content_type: String,
    #[serde(default)]
    filename: String,
    #[serde(default)]
    file_size: i64,
    #[serde(default)]
    node_id: String,
    #[serde(default)]
    relay_url: String,
    /// v2: list of category strings (e.g. ["action", "indie"])
    #[serde(default)]
    categories: Vec<String>,
}

#[derive(Serialize)]
pub struct SyncResult {
    pub new_content: u32,
    pub delisted_content: u32,
    pub synced_to_block: u64,
}

#[derive(Serialize)]
pub struct RewardSyncResult {
    pub distributions_found: u32,
    pub claims_found: u32,
    pub purchases_found: u32,
    pub synced_to_block: u64,
}

/// Which event source to fetch from.
enum EventSource {
    Content,
    Marketplace,
}

/// Fetch events in chunks, adapting chunk size to RPC limits.
/// Starts with a large chunk and halves on range errors.
/// Retries with exponential backoff on 429 rate-limit responses.
async fn fetch_events_chunked(
    state: &AppState,
    from_block: u64,
    to_block: u64,
    source: EventSource,
) -> Result<Vec<IndexedEvent>, String> {
    let chain = state
        .chain_client()
        .map_err(|e| format!("Chain client error: {e}"))?;

    let mut all_events = Vec::new();
    let mut cursor = from_block;
    let mut chunk_size: u64 = 2000;
    let mut rate_limit_retries: u32 = 0;
    const MAX_RATE_LIMIT_RETRIES: u32 = 5;

    while cursor <= to_block {
        let chunk_end = (cursor + chunk_size - 1).min(to_block);

        let result = match source {
            EventSource::Content => {
                chain.events.fetch_content_events(cursor, Some(chunk_end)).await
            }
            EventSource::Marketplace => {
                chain.events.fetch_marketplace_events(cursor, Some(chunk_end)).await
            }
        };

        match result {
            Ok(events) => {
                all_events.extend(events);
                cursor = chunk_end + 1;
                rate_limit_retries = 0;
                // If we succeeded with a small chunk, try growing back
                if chunk_size < 2000 {
                    chunk_size = (chunk_size * 2).min(2000);
                }
            }
            Err(e) => {
                let err_msg = e.to_string();
                // Rate limit (429) — back off and retry
                if err_msg.contains("429") || err_msg.contains("compute units") {
                    if rate_limit_retries >= MAX_RATE_LIMIT_RETRIES {
                        return Err(format!(
                            "RPC rate limit: gave up after {MAX_RATE_LIMIT_RETRIES} retries at block {cursor}"
                        ));
                    }
                    rate_limit_retries += 1;
                    let delay =
                        std::time::Duration::from_millis(500 * 2u64.pow(rate_limit_retries));
                    info!(
                        "RPC rate limited, waiting {}ms before retry {}/{}",
                        delay.as_millis(),
                        rate_limit_retries,
                        MAX_RATE_LIMIT_RETRIES
                    );
                    tokio::time::sleep(delay).await;
                    continue; // Retry same cursor
                }
                // RPC block range limit — shrink chunk and retry
                if err_msg.contains("block range") || err_msg.contains("-32600") {
                    if chunk_size <= 1 {
                        return Err(format!(
                            "RPC rejects even single-block queries at block {cursor}: {err_msg}"
                        ));
                    }
                    chunk_size = (chunk_size / 2).max(1);
                    info!(
                        "RPC block range limit hit, reducing chunk to {} blocks",
                        chunk_size
                    );
                    continue; // Retry same cursor with smaller chunk
                }
                return Err(format!("Event fetch failed at block {cursor}: {err_msg}"));
            }
        }
    }

    Ok(all_events)
}

/// Core sync logic — usable from both the Tauri command and app startup.
pub async fn sync_content_impl(state: &AppState) -> Result<SyncResult, String> {
    let chain = state
        .chain_client()
        .map_err(|e| format!("Chain client error: {e}"))?;

    // Determine block range
    let from_block = {
        let db = state.db.lock().await;
        db.get_config("last_synced_block")
            .and_then(|s| s.parse::<u64>().ok())
            .map(|b| b + 1) // Start from next unsynced block
            .unwrap_or(state.config.ethereum.deployment_block)
    };

    let to_block = chain
        .get_block_number()
        .await
        .map_err(|e| format!("Failed to get block number: {e}"))?;

    if from_block > to_block {
        return Ok(SyncResult {
            new_content: 0,
            delisted_content: 0,
            synced_to_block: to_block,
        });
    }

    let total_blocks = to_block - from_block + 1;
    info!(
        "Syncing content events from block {} to {} ({} blocks)",
        from_block, to_block, total_blocks
    );

    // Fetch events in adaptive chunks (handles RPC range limits)
    let events = fetch_events_chunked(state, from_block, to_block, EventSource::Content).await?;

    let mut new_count = 0u32;
    let mut delisted_count = 0u32;

    let db = state.db.lock().await;
    for indexed in &events {
        match &indexed.event {
            AraEvent::ContentPublished {
                content_id,
                creator,
                content_hash,
                metadata_uri,
                price_wei,
            } => {
                let cid = format!("0x{}", alloy::hex::encode(content_id.as_slice()));
                let chash = format!("0x{}", alloy::hex::encode(content_hash.as_slice()));
                let creator_str = format!("{creator:#x}");

                // Parse metadata JSON; fall back to empty fields for legacy ara://local/ URIs
                let meta: MetadataV1 = match serde_json::from_str(metadata_uri) {
                    Ok(m) => m,
                    Err(e) => {
                        warn!(
                            "Failed to parse metadata_uri for content {}: {} (raw: {:?})",
                            cid, e, metadata_uri
                        );
                        MetadataV1 {
                            title: String::new(),
                            description: String::new(),
                            content_type: String::new(),
                            filename: String::new(),
                            file_size: 0,
                            node_id: String::new(),
                            relay_url: String::new(),
                            categories: Vec::new(),
                        }
                    }
                };

                let created_at = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64;

                let categories_json = serde_json::to_string(&meta.categories).unwrap_or_default();
                match db.upsert_synced_content(
                    &cid,
                    &chash,
                    &creator_str,
                    metadata_uri,
                    &price_wei.to_string(),
                    &meta.title,
                    &meta.description,
                    &meta.content_type,
                    &meta.filename,
                    meta.file_size,
                    &meta.node_id,
                    &meta.relay_url,
                    created_at,
                    &categories_json,
                ) {
                    Ok(n) if n > 0 => {
                        new_count += 1;
                        info!("Synced content: {} ({})", meta.title, cid);
                    }
                    Err(e) => warn!("Failed to upsert content {}: {}", cid, e),
                    _ => {}
                }
            }
            AraEvent::ContentDelisted { content_id } => {
                let cid = format!("0x{}", alloy::hex::encode(content_id.as_slice()));
                let _ = db.conn().execute(
                    "UPDATE content SET active = 0 WHERE content_id = ?1",
                    rusqlite::params![&cid],
                );
                delisted_count += 1;
            }
            AraEvent::ContentUpdated {
                content_id,
                new_price_wei,
                new_metadata_uri,
            } => {
                let cid = format!("0x{}", alloy::hex::encode(content_id.as_slice()));

                // Parse updated metadata JSON to extract title, description, etc.
                let meta: MetadataV1 = match serde_json::from_str(new_metadata_uri) {
                    Ok(m) => m,
                    Err(e) => {
                        warn!(
                            "Failed to parse updated metadata_uri for {}: {} (raw: {:?})",
                            cid, e, new_metadata_uri
                        );
                        // Still update price and raw metadata_uri even if parsing fails
                        let _ = db.conn().execute(
                            "UPDATE content SET price_wei = ?1, metadata_uri = ?2 WHERE content_id = ?3",
                            rusqlite::params![&new_price_wei.to_string(), new_metadata_uri, &cid],
                        );
                        continue;
                    }
                };

                let categories_json = serde_json::to_string(&meta.categories).unwrap_or_default();
                let _ = db.conn().execute(
                    "UPDATE content SET price_wei = ?1, metadata_uri = ?2,
                     title = CASE WHEN ?3 != '' THEN ?3 ELSE title END,
                     description = CASE WHEN ?4 != '' THEN ?4 ELSE description END,
                     content_type = CASE WHEN ?5 != '' THEN ?5 ELSE content_type END,
                     filename = CASE WHEN ?6 != '' THEN ?6 ELSE filename END,
                     file_size_bytes = CASE WHEN ?7 > 0 THEN ?7 ELSE file_size_bytes END,
                     publisher_node_id = CASE WHEN ?8 != '' THEN ?8 ELSE publisher_node_id END,
                     publisher_relay_url = CASE WHEN ?9 != '' THEN ?9 ELSE publisher_relay_url END,
                     categories = CASE WHEN ?11 != '' THEN ?11 ELSE categories END
                     WHERE content_id = ?10",
                    rusqlite::params![
                        &new_price_wei.to_string(),
                        new_metadata_uri,
                        &meta.title,
                        &meta.description,
                        &meta.content_type,
                        &meta.filename,
                        meta.file_size,
                        &meta.node_id,
                        &meta.relay_url,
                        &cid,
                        &categories_json,
                    ],
                );
                info!("Updated content metadata: {} ({})", meta.title, cid);
            }
            _ => {}
        }
    }

    // Save sync progress
    let _ = db.set_config("last_synced_block", &to_block.to_string());
    drop(db);

    info!(
        "Sync complete: {} new, {} delisted, synced to block {}",
        new_count, delisted_count, to_block
    );

    Ok(SyncResult {
        new_content: new_count,
        delisted_content: delisted_count,
        synced_to_block: to_block,
    })
}

/// Sync marketplace events (purchases, distributions, claims) for the connected wallet.
/// Rebuilds the `rewards` and `purchases` tables from on-chain events.
pub async fn sync_rewards_impl(state: &AppState) -> Result<RewardSyncResult, String> {
    let wallet = state.wallet_address.lock().await;
    let wallet_str = wallet.as_ref().ok_or("No wallet connected")?.clone();
    let wallet_addr: Address = wallet_str
        .parse()
        .map_err(|e| format!("Invalid wallet address: {e}"))?;
    drop(wallet);

    let chain = state
        .chain_client()
        .map_err(|e| format!("Chain client error: {e}"))?;

    let from_block = {
        let db = state.db.lock().await;
        db.get_config("rewards_sync_block")
            .and_then(|s| s.parse::<u64>().ok())
            .map(|b| b + 1)
            .unwrap_or(state.config.ethereum.deployment_block)
    };

    let to_block = chain
        .get_block_number()
        .await
        .map_err(|e| format!("Failed to get block number: {e}"))?;

    if from_block > to_block {
        return Ok(RewardSyncResult {
            distributions_found: 0,
            claims_found: 0,
            purchases_found: 0,
            synced_to_block: to_block,
        });
    }

    info!(
        "Syncing marketplace events for {} from block {} to {}",
        wallet_str, from_block, to_block
    );

    let events = fetch_events_chunked(state, from_block, to_block, EventSource::Marketplace).await?;

    let mut distributions_found = 0u32;
    let mut claims_found = 0u32;
    let mut purchases_found = 0u32;

    let db = state.db.lock().await;
    for indexed in &events {
        let tx_hash_str = indexed
            .tx_hash
            .map(|h| format!("0x{}", alloy::hex::encode(h.as_slice())))
            .unwrap_or_default();

        // Use block number as approximate timestamp (precise timestamps would require
        // fetching each block header, which is expensive). Good enough for sorting.
        let approx_timestamp = indexed.block_number as i64;

        match &indexed.event {
            AraEvent::ContentPurchased {
                content_id,
                buyer,
                price_paid,
                ..
            } => {
                if *buyer == wallet_addr {
                    let cid = format!("0x{}", alloy::hex::encode(content_id.as_slice()));
                    let buyer_str = format!("{buyer:#x}");
                    if let Err(e) = db.upsert_purchase(
                        &cid,
                        &buyer_str,
                        &price_paid.to_string(),
                        &tx_hash_str,
                        approx_timestamp,
                    ) {
                        warn!("Failed to upsert purchase {}: {}", cid, e);
                    } else {
                        purchases_found += 1;
                    }
                }
            }
            AraEvent::RewardsDistributed {
                content_id,
                seeders,
                amounts,
                ..
            } => {
                // Find this wallet's share
                for (i, seeder) in seeders.iter().enumerate() {
                    if *seeder == wallet_addr {
                        let cid = format!("0x{}", alloy::hex::encode(content_id.as_slice()));
                        let amount = amounts.get(i).copied().unwrap_or_default();
                        if let Err(e) = db.insert_reward(
                            &cid,
                            &amount.to_string(),
                            &tx_hash_str,
                            approx_timestamp,
                        ) {
                            warn!("Failed to insert reward for {}: {}", cid, e);
                        } else {
                            distributions_found += 1;
                        }
                    }
                }
            }
            AraEvent::RewardClaimed { seeder, amount } => {
                if *seeder == wallet_addr {
                    if let Err(e) = db.insert_reward_claim(
                        &amount.to_string(),
                        &tx_hash_str,
                        approx_timestamp,
                    ) {
                        warn!("Failed to insert reward claim: {}", e);
                    } else {
                        claims_found += 1;
                    }
                }
            }
            _ => {}
        }
    }

    let _ = db.set_config("rewards_sync_block", &to_block.to_string());
    drop(db);

    info!(
        "Rewards sync complete: {} distributions, {} claims, {} purchases, synced to block {}",
        distributions_found, claims_found, purchases_found, to_block
    );

    Ok(RewardSyncResult {
        distributions_found,
        claims_found,
        purchases_found,
        synced_to_block: to_block,
    })
}

#[tauri::command]
pub async fn sync_content(state: State<'_, AppState>) -> Result<SyncResult, String> {
    sync_content_impl(&state).await
}

#[tauri::command]
pub async fn sync_rewards(state: State<'_, AppState>) -> Result<RewardSyncResult, String> {
    sync_rewards_impl(&state).await
}
