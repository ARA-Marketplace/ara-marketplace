use crate::state::AppState;
use ara_chain::{AraEvent, IndexedEvent};
use serde::{Deserialize, Serialize};
use tauri::State;
use tracing::{info, warn};

/// Metadata embedded in the on-chain metadataURI (v1 JSON format).
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
}

#[derive(Serialize)]
pub struct SyncResult {
    pub new_content: u32,
    pub delisted_content: u32,
    pub synced_to_block: u64,
}

/// Fetch content events in chunks, adapting chunk size to RPC limits.
/// Starts with a large chunk and halves on range errors.
/// Retries with exponential backoff on 429 rate-limit responses.
async fn fetch_events_chunked(
    state: &AppState,
    from_block: u64,
    to_block: u64,
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

        match chain
            .events
            .fetch_content_events(cursor, Some(chunk_end))
            .await
        {
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
    let events = fetch_events_chunked(state, from_block, to_block).await?;

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
                let meta: MetadataV1 = serde_json::from_str(metadata_uri).unwrap_or(MetadataV1 {
                    title: String::new(),
                    description: String::new(),
                    content_type: String::new(),
                    filename: String::new(),
                    file_size: 0,
                    node_id: String::new(),
                });

                let created_at = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64;

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
                    created_at,
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
                let _ = db.conn().execute(
                    "UPDATE content SET price_wei = ?1, metadata_uri = ?2 WHERE content_id = ?3",
                    rusqlite::params![&new_price_wei.to_string(), new_metadata_uri, &cid],
                );
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

#[tauri::command]
pub async fn sync_content(state: State<'_, AppState>) -> Result<SyncResult, String> {
    sync_content_impl(&state).await
}
