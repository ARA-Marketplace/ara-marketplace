use crate::gossip_actor::GossipCmd;
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use tauri::State;
use tracing::info;

#[derive(Serialize, Deserialize)]
pub struct SeederStats {
    pub content_id: String,
    pub title: String,
    pub bytes_served: u64,
    pub peer_count: u32,
    pub ara_staked: String,
    pub is_active: bool,
}

/// Start seeding a content item. The blob is already in iroh's store
/// (imported during publish or downloaded during purchase).
/// This records the seeding state in the local DB.
#[tauri::command]
pub async fn start_seeding(
    state: State<'_, AppState>,
    content_id: String,
) -> Result<(), String> {
    info!("Starting to seed: {}", content_id);

    let started_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    {
        let db = state.db.lock().await;
        db.conn()
            .execute(
                "INSERT OR REPLACE INTO seeding (content_id, active, bytes_served, peer_count, started_at)
                 VALUES (?1, 1, COALESCE((SELECT bytes_served FROM seeding WHERE content_id = ?1), 0),
                         COALESCE((SELECT peer_count FROM seeding WHERE content_id = ?1), 0), ?2)",
                rusqlite::params![&content_id, started_at],
            )
            .map_err(|e| format!("DB update failed: {e}"))?;
    }

    // Ensure iroh is running (lazy start) so gossip actor is available
    let _ = state.ensure_iroh().await?;

    // Look up the BLAKE3 content_hash and publisher_node_id from the content table.
    // The gossip topic MUST use the BLAKE3 hash (not the keccak256 content_id).
    let (content_hash_hex, publisher_node_id_opt): (String, Option<String>) = {
        let db = state.db.lock().await;
        db.conn()
            .query_row(
                "SELECT content_hash, publisher_node_id FROM content WHERE content_id = ?1",
                rusqlite::params![&content_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| format!("Content not found: {e}"))?
    };

    let content_hash = parse_content_hash(&content_hash_hex)?;

    // Build bootstrap list: use the publisher's NodeId if known
    let bootstrap: Vec<iroh::NodeId> = publisher_node_id_opt
        .as_deref()
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse::<iroh::NodeId>().ok())
        .into_iter()
        .collect();

    state
        .send_gossip(GossipCmd::AnnounceSeeding {
            content_hash,
            bootstrap,
        })
        .await?;

    info!("Seeding started for {}", content_id);
    Ok(())
}

/// Stop seeding a content item.
#[tauri::command]
pub async fn stop_seeding(
    state: State<'_, AppState>,
    content_id: String,
) -> Result<(), String> {
    info!("Stopping seed: {}", content_id);

    {
        let db = state.db.lock().await;
        db.conn()
            .execute(
                "UPDATE seeding SET active = 0 WHERE content_id = ?1",
                rusqlite::params![&content_id],
            )
            .map_err(|e| format!("DB update failed: {e}"))?;
    }

    // Look up BLAKE3 content_hash (gossip topics use BLAKE3, not keccak256 content_id)
    let content_hash_hex: String = {
        let db = state.db.lock().await;
        db.conn()
            .query_row(
                "SELECT content_hash FROM content WHERE content_id = ?1",
                rusqlite::params![&content_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("Content not found: {e}"))?
    };
    let content_hash = parse_content_hash(&content_hash_hex)?;

    // Leave gossip topic
    state
        .send_gossip(GossipCmd::LeaveSeeding {
            content_hash,
        })
        .await?;

    info!("Seeding stopped for {}", content_id);
    Ok(())
}

/// Get seeding stats for all content the user is seeding.
/// Supplements DB-stored peer_count with live data from the gossip overlay.
#[tauri::command]
pub async fn get_seeder_stats(
    state: State<'_, AppState>,
) -> Result<Vec<SeederStats>, String> {
    info!("Fetching seeder stats");

    // Collect DB rows, including the BLAKE3 content_hash for gossip peer lookup
    let rows_from_db: Vec<(SeederStats, String)> = {
        let db = state.db.lock().await;
        let conn = db.conn();

        let mut stmt = conn
            .prepare(
                "SELECT s.content_id, c.title, s.bytes_served, s.peer_count, s.active, c.content_hash
                 FROM seeding s
                 LEFT JOIN content c ON s.content_id = c.content_id
                 ORDER BY s.started_at DESC",
            )
            .map_err(|e| format!("DB query failed: {e}"))?;

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    SeederStats {
                        content_id: row.get(0)?,
                        title: row.get::<_, Option<String>>(1)?.unwrap_or("Unknown".to_string()),
                        // NOTE: bytes_served stays at 0 because iroh serves blobs transparently
                        // without per-upload byte callbacks. When iroh exposes upload metrics,
                        // integrate with ara_p2p::metrics::MetricsTracker.
                        bytes_served: row.get::<_, i64>(2)? as u64,
                        peer_count: row.get::<_, i32>(3)? as u32,
                        ara_staked: "0.0".to_string(), // TODO: query staking contract
                        is_active: row.get::<_, i32>(4)? != 0,
                    },
                    row.get::<_, Option<String>>(5)?.unwrap_or_default(), // BLAKE3 content_hash
                ))
            })
            .map_err(|e| format!("DB query failed: {e}"))?;

        let mut collected = Vec::new();
        for row in rows {
            collected.push(row.map_err(|e| format!("Row parse error: {e}"))?);
        }
        collected
    };

    // Supplement peer_count with live gossip data from known_seeders
    let known_seeders = state.known_seeders.lock().await;

    let mut items = Vec::new();
    for (mut stats, content_hash_hex) in rows_from_db {
        if stats.is_active && !content_hash_hex.is_empty() {
            if let Ok(hash_bytes) = parse_content_hash(&content_hash_hex) {
                if let Some(peers) = known_seeders.get(&hash_bytes) {
                    let live_count = peers.len() as u32;
                    if live_count > stats.peer_count {
                        stats.peer_count = live_count;
                    }
                }
            }
        }
        items.push(stats);
    }

    info!("Seeder stats: {} items", items.len());
    Ok(items)
}

/// Parse a 0x-prefixed hex content ID into a 32-byte hash.
fn parse_content_hash(content_id: &str) -> Result<[u8; 32], String> {
    let hex_str = content_id.strip_prefix("0x").unwrap_or(content_id);
    let bytes =
        alloy::hex::decode(hex_str).map_err(|e| format!("Invalid content hash: {e}"))?;
    if bytes.len() != 32 {
        return Err(format!(
            "Content hash must be 32 bytes, got {}",
            bytes.len()
        ));
    }
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&bytes);
    Ok(hash)
}
