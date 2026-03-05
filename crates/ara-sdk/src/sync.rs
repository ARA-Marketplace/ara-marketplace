use alloy::providers::Provider;
use anyhow::Result;
use tracing::{info, warn};

use ara_chain::{AraChain, AraEvent, IndexedEvent};

use crate::client::AraClient;
use crate::types::SyncResult;

/// Sync operations: pull events from chain into local database.
pub struct SyncOps<'a> {
    pub(crate) client: &'a AraClient,
}

/// Metadata embedded in the on-chain metadataURI (v1/v2/v3 JSON format).
#[derive(serde::Deserialize)]
struct ContentMetadata {
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
    #[serde(default)]
    categories: Vec<String>,
}

impl SyncOps<'_> {
    /// Sync content events from chain to local DB. Returns a summary of changes.
    pub async fn sync_content(&self) -> Result<SyncResult> {
        let chain = self.client.chain_client()?;
        let current_block = chain.get_block_number().await?;

        let db = self.client.db.lock().await;
        let last_synced: u64 = db
            .get_config("last_synced_block")
            .and_then(|s| s.parse().ok())
            .unwrap_or(self.client.config.ethereum.deployment_block);
        drop(db);

        if last_synced >= current_block {
            return Ok(SyncResult {
                new_content: 0,
                updated: 0,
                delisted: 0,
                from_block: last_synced,
                to_block: current_block,
            });
        }

        let from = last_synced + 1;
        info!("Syncing content events from block {} to {}", from, current_block);

        let events = fetch_events_chunked(&chain, from, current_block).await?;

        let mut new_content = 0u32;
        let mut updated = 0u32;
        let mut delisted = 0u32;

        let db = self.client.db.lock().await;

        for indexed in &events {
            match &indexed.event {
                AraEvent::ContentPublished {
                    content_id,
                    creator,
                    content_hash,
                    metadata_uri,
                    price_wei,
                    ..
                } => {
                    let meta: ContentMetadata =
                        serde_json::from_str(metadata_uri).unwrap_or_else(|_| ContentMetadata {
                            title: String::new(),
                            description: String::new(),
                            content_type: String::new(),
                            filename: String::new(),
                            file_size: 0,
                            node_id: String::new(),
                            relay_url: String::new(),
                            categories: vec![],
                        });

                    let cats_json =
                        serde_json::to_string(&meta.categories).unwrap_or_else(|_| "[]".to_string());

                    db.upsert_synced_content(
                        &format!("{content_id:#x}"),
                        &format!("{content_hash:#x}"),
                        &format!("{creator:#x}"),
                        metadata_uri,
                        &price_wei.to_string(),
                        &meta.title,
                        &meta.description,
                        &meta.content_type,
                        &meta.filename,
                        meta.file_size,
                        &meta.node_id,
                        &meta.relay_url,
                        0, // created_at — not available from event
                        &cats_json,
                        0,
                        0,
                        "",
                    )?;
                    new_content += 1;
                }
                AraEvent::ContentDelisted { content_id, .. } => {
                    let id_str = format!("{content_id:#x}");
                    let _ = db.conn().execute(
                        "UPDATE content SET active = 0 WHERE content_id = ?1",
                        rusqlite::params![&id_str],
                    );
                    delisted += 1;
                }
                AraEvent::ContentUpdated {
                    content_id,
                    new_price_wei,
                    new_metadata_uri,
                    ..
                } => {
                    let id_str = format!("{content_id:#x}");
                    let _ = db.conn().execute(
                        "UPDATE content SET price_wei = ?1, metadata_uri = ?2 WHERE content_id = ?3",
                        rusqlite::params![&new_price_wei.to_string(), new_metadata_uri, &id_str],
                    );
                    updated += 1;
                }
                _ => {}
            }
        }

        db.set_config("last_synced_block", &current_block.to_string())?;
        drop(db);

        info!(
            "Sync complete: {} new, {} updated, {} delisted",
            new_content, updated, delisted
        );

        Ok(SyncResult {
            new_content,
            updated,
            delisted,
            from_block: from,
            to_block: current_block,
        })
    }
}

/// Fetch events in chunks, halving chunk size on range errors.
async fn fetch_events_chunked<P: Provider + Clone>(
    chain: &AraChain<P>,
    from: u64,
    to: u64,
) -> Result<Vec<IndexedEvent>> {
    let mut all_events = Vec::new();
    let mut chunk_size: u64 = 10_000;
    let mut current = from;

    while current <= to {
        let end = (current + chunk_size - 1).min(to);
        match chain.events.fetch_events(current, Some(end)).await {
            Ok(events) => {
                all_events.extend(events);
                current = end + 1;
            }
            Err(e) => {
                let msg = format!("{e}");
                if msg.contains("range") || msg.contains("block range") || msg.contains("10000")
                {
                    chunk_size = (chunk_size / 2).max(100);
                    warn!("Reducing chunk size to {} due to range limit", chunk_size);
                } else {
                    return Err(e);
                }
            }
        }
    }

    Ok(all_events)
}
