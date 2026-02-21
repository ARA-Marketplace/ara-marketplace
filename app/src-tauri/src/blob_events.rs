//! iroh-blobs event handler for tracking bytes served to remote peers.
//!
//! `TransferBlobCompleted` only fires for *child blobs inside a hashseq* (offset > 0).
//! Raw blob transfers (BlobFormat::Raw, the common case) only touch offset 0 and
//! therefore only emit `GetRequestReceived` + `TransferCompleted`.
//!
//! Strategy: pair the two reliable events using (connection_id, request_id):
//!   1. `GetRequestReceived { hash, connection_id, request_id }` → store hash in pending map
//!   2. `TransferCompleted { connection_id, request_id, stats }` → look up hash, update DB

use std::collections::HashMap;
use std::sync::Arc;

use ara_core::storage::Database;
use futures_lite::future::Boxed as BoxFuture;
use iroh_blobs::provider::{CustomEventSender, Event};
use tauri::Emitter;
use tokio::sync::Mutex;
use tracing::{info, warn};

/// Pending request map: (connection_id, request_id) → blob hash bytes
type PendingMap = Arc<Mutex<HashMap<(u64, u64), [u8; 32]>>>;

pub struct BlobTransferSender {
    pub db: Arc<Mutex<Database>>,
    pub app_handle: tauri::AppHandle,
    /// Tracks in-flight requests so we can correlate GetRequestReceived → TransferCompleted
    pending: PendingMap,
}

impl BlobTransferSender {
    pub fn new(db: Arc<Mutex<Database>>, app_handle: tauri::AppHandle) -> Self {
        Self {
            db,
            app_handle,
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl std::fmt::Debug for BlobTransferSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlobTransferSender").finish()
    }
}

impl CustomEventSender for BlobTransferSender {
    /// Called for reliable (awaited) transfer lifecycle events.
    fn send(&self, event: Event) -> BoxFuture<()> {
        let db = self.db.clone();
        let app_handle = self.app_handle.clone();
        let pending = self.pending.clone();

        Box::pin(async move {
            match event {
                // Step 1: record hash for this in-flight request
                Event::GetRequestReceived {
                    hash,
                    connection_id,
                    request_id,
                } => {
                    let mut map = pending.lock().await;
                    map.insert((connection_id, request_id), *hash.as_bytes());
                }

                // Step 2: transfer done — look up hash, update bytes_served
                Event::TransferCompleted {
                    connection_id,
                    request_id,
                    stats,
                } => {
                    let hash_bytes = {
                        let mut map = pending.lock().await;
                        map.remove(&(connection_id, request_id))
                    };

                    if let Some(hash_bytes) = hash_bytes {
                        let bytes_sent = stats.send.total().size;
                        let hash_hex = format!("0x{}", alloy::hex::encode(hash_bytes));
                        info!("Blob transfer completed: {} ({} bytes sent)", hash_hex, bytes_sent);

                        // Use IN (not =) so multiple content rows with the same content_hash
                        // (temp active=0 + synced active=1) don't cause "sub-select returns
                        // more than 1 row" errors.
                        let update_result = {
                            let db = db.lock().await;
                            db.conn().execute(
                                "UPDATE seeding SET bytes_served = bytes_served + ?1
                                 WHERE content_id IN (SELECT content_id FROM content WHERE content_hash = ?2)",
                                rusqlite::params![bytes_sent as i64, &hash_hex],
                            )
                        };

                        match update_result {
                            Ok(n) if n > 0 => {
                                let _ = app_handle.emit("seeder-stats-updated", ());
                            }
                            Ok(_) => {
                                // No seeding row found. This happens when confirm_publish was
                                // skipped or failed (e.g. sync raced ahead, causing a UNIQUE
                                // conflict in confirm_publish). Auto-create a seeding entry if
                                // the content is already active (synced from chain).
                                let auto_created = {
                                    let db = db.lock().await;
                                    let conn = db.conn();
                                    let content_id_opt: Option<String> = conn.query_row(
                                        "SELECT content_id FROM content WHERE content_hash = ?1 AND active = 1",
                                        rusqlite::params![&hash_hex],
                                        |row| row.get(0),
                                    ).ok();
                                    if let Some(content_id) = content_id_opt {
                                        let now = std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap()
                                            .as_secs() as i64;
                                        // Create seeding entry if not present
                                        let _ = conn.execute(
                                            "INSERT OR IGNORE INTO seeding
                                             (content_id, active, bytes_served, peer_count, started_at)
                                             VALUES (?1, 1, 0, 0, ?2)",
                                            rusqlite::params![&content_id, now],
                                        );
                                        // Credit the bytes
                                        let _ = conn.execute(
                                            "UPDATE seeding SET bytes_served = bytes_served + ?1
                                             WHERE content_id = ?2",
                                            rusqlite::params![bytes_sent as i64, &content_id],
                                        );
                                        info!(
                                            "Auto-created seeding entry for {} (content_id={})",
                                            hash_hex, content_id
                                        );
                                        true
                                    } else {
                                        false
                                    }
                                };
                                if auto_created {
                                    let _ = app_handle.emit("seeder-stats-updated", ());
                                }
                                // else: iroh internal metadata blob — ignore silently
                            }
                            Err(e) => {
                                warn!("Failed to update bytes_served for {}: {e}", hash_hex);
                            }
                        }
                    }
                }

                // Clean up pending map if transfer aborted (no bytes served)
                Event::TransferAborted {
                    connection_id,
                    request_id,
                    ..
                } => {
                    let mut map = pending.lock().await;
                    map.remove(&(connection_id, request_id));
                }

                _ => {}
            }
        })
    }

    /// Called for high-frequency progress events (non-blocking, can be dropped).
    fn try_send(&self, _event: Event) {}
}
