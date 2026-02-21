//! iroh-blobs event handler for tracking bytes served to remote peers.
//!
//! Implements `CustomEventSender` so that every time a blob transfer completes
//! (i.e. a remote peer has fully downloaded a blob from us), we increment
//! `bytes_served` in the local DB for the matching content item.

use std::sync::Arc;

use ara_core::storage::Database;
use futures_lite::future::Boxed as BoxFuture;
use iroh_blobs::provider::{CustomEventSender, Event};
use tauri::Emitter;
use tokio::sync::Mutex;
use tracing::{info, warn};

pub struct BlobTransferSender {
    pub db: Arc<Mutex<Database>>,
    pub app_handle: tauri::AppHandle,
}

impl std::fmt::Debug for BlobTransferSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlobTransferSender").finish()
    }
}

impl CustomEventSender for BlobTransferSender {
    /// Called when a transfer event occurs (reliable, awaited by iroh).
    /// We only act on `TransferBlobCompleted` — the moment a full blob has
    /// been sent to a remote peer.
    fn send(&self, event: Event) -> BoxFuture<()> {
        let db = self.db.clone();
        let app_handle = self.app_handle.clone();
        Box::pin(async move {
            if let Event::TransferBlobCompleted { hash, size, .. } = event {
                let hash_hex = format!("0x{}", alloy::hex::encode(hash.as_bytes()));
                info!("Blob transfer completed: {} ({} bytes)", hash_hex, size);

                let updated = {
                    let db = db.lock().await;
                    db.conn().execute(
                        "UPDATE seeding SET bytes_served = bytes_served + ?1
                         WHERE content_id = (SELECT content_id FROM content WHERE content_hash = ?2)",
                        rusqlite::params![size as i64, &hash_hex],
                    )
                };

                match updated {
                    Ok(n) if n > 0 => {
                        let _ = app_handle.emit("seeder-stats-updated", ());
                    }
                    Ok(_) => {
                        // Hash not in our content table — could be iroh internal metadata blob
                    }
                    Err(e) => {
                        warn!("Failed to update bytes_served for {}: {e}", hash_hex);
                    }
                }
            }
        })
    }

    /// Called for high-frequency progress events (non-blocking, can be dropped).
    /// We ignore these — the completed event is sufficient for accounting.
    fn try_send(&self, _event: Event) {}
}
