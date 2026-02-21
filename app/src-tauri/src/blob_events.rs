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

                        let updated = {
                            let db = db.lock().await;
                            db.conn().execute(
                                "UPDATE seeding SET bytes_served = bytes_served + ?1
                                 WHERE content_id = (SELECT content_id FROM content WHERE content_hash = ?2)",
                                rusqlite::params![bytes_sent as i64, &hash_hex],
                            )
                        };

                        match updated {
                            Ok(n) if n > 0 => {
                                let _ = app_handle.emit("seeder-stats-updated", ());
                            }
                            Ok(_) => {
                                // Hash not in content table — could be iroh internal metadata blob
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
