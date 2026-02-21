use alloy::primitives::Address;
use ara_chain::{connect_http, AraChain, ContractAddresses};
use ara_core::config::AppConfig;
use ara_core::storage::Database;
use ara_p2p::node::IrohNode;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::gossip_actor::{self, GossipCmd, KnownSeeders};

/// Application state shared across all Tauri commands.
pub struct AppState {
    pub config: AppConfig,
    pub db: Arc<Mutex<Database>>,
    pub iroh_node: Arc<Mutex<Option<IrohNode>>>,
    pub wallet_address: Arc<Mutex<Option<String>>>,
    pub gossip_tx: Arc<Mutex<Option<tokio::sync::mpsc::Sender<GossipCmd>>>>,
    pub known_seeders: KnownSeeders,
    pub app_handle: tauri::AppHandle,
}

impl AppState {
    pub fn new(config: AppConfig, db: Database, app_handle: tauri::AppHandle) -> Self {
        Self {
            config,
            db: Arc::new(Mutex::new(db)),
            iroh_node: Arc::new(Mutex::new(None)),
            wallet_address: Arc::new(Mutex::new(None)),
            gossip_tx: Arc::new(Mutex::new(None)),
            known_seeders: Arc::new(Mutex::new(std::collections::HashMap::new())),
            app_handle,
        }
    }

    /// Ensure the iroh P2P node is started (lazy init).
    /// Returns a MutexGuard with a guaranteed `Some(IrohNode)`.
    /// Callers should extract what they need (e.g. `blobs_client()`)
    /// and drop the guard before doing async work.
    pub async fn ensure_iroh(
        &self,
    ) -> Result<tokio::sync::MutexGuard<'_, Option<IrohNode>>, String> {
        let mut guard = self.iroh_node.lock().await;
        if guard.is_none() {
            let data_dir = std::path::Path::new(&self.config.iroh.data_dir);
            std::fs::create_dir_all(data_dir)
                .map_err(|e| format!("Failed to create iroh data dir: {e}"))?;
            info!("Starting iroh P2P node at {:?}...", data_dir);
            let node = IrohNode::start(data_dir)
                .await
                .map_err(|e| format!("Failed to start iroh node: {e}"))?;
            info!("Iroh P2P node started: {}", node.node_id());

            // Spawn gossip actor (if not already running)
            let mut gossip_guard = self.gossip_tx.lock().await;
            if gossip_guard.is_none() {
                let gossip = node.gossip().clone();
                let node_id = node.node_id();
                let tx =
                    gossip_actor::spawn(gossip, node_id, self.known_seeders.clone(), self.app_handle.clone(), self.db.clone());
                *gossip_guard = Some(tx.clone());
                info!("Gossip actor spawned");

                // Resume seeding announcements for content that was active before restart.
                // Clone the endpoint so we can add relay URL hints before each gossip join.
                let endpoint = node.endpoint().clone();
                let db = self.db.clone();
                tokio::spawn(async move {
                    resume_active_seeding(db, tx, endpoint).await;
                });
            }
            drop(gossip_guard);

            *guard = Some(node);
        }
        Ok(guard)
    }

    /// Send a command to the gossip actor. No-op if iroh hasn't started yet.
    pub async fn send_gossip(&self, cmd: GossipCmd) -> Result<(), String> {
        let guard = self.gossip_tx.lock().await;
        if let Some(tx) = guard.as_ref() {
            tx.send(cmd)
                .await
                .map_err(|e| format!("Gossip actor send failed: {e}"))?;
        }
        Ok(())
    }

    /// Create an AraChain client from the current config.
    /// HTTP providers are stateless, so this is cheap to call per-request.
    pub fn chain_client(
        &self,
    ) -> Result<AraChain<impl alloy::providers::Provider + Clone>, String> {
        let eth = &self.config.ethereum;

        let addresses = ContractAddresses {
            ara_token: parse_address(&eth.ara_token_address, "ara_token")?,
            staking: parse_address(&eth.staking_address, "staking")?,
            registry: parse_address(&eth.registry_address, "registry")?,
            marketplace: parse_address(&eth.marketplace_address, "marketplace")?,
        };

        connect_http(&eth.rpc_url, addresses).map_err(|e| format!("Chain connect failed: {e}"))
    }
}

fn parse_address(s: &str, name: &str) -> Result<Address, String> {
    if s.is_empty() {
        // Return zero address for contracts not yet deployed
        return Ok(Address::ZERO);
    }
    s.parse::<Address>()
        .map_err(|e| format!("Invalid {name} address '{s}': {e}"))
}

/// Re-announce seeding on gossip for all content that was actively seeded before app restart.
/// Also explicitly adds each bootstrap peer's relay URL to the endpoint routing table so
/// gossip can dial them even if we've never connected to them in this session.
async fn resume_active_seeding(
    db: Arc<Mutex<Database>>,
    gossip_tx: tokio::sync::mpsc::Sender<GossipCmd>,
    endpoint: iroh::Endpoint,
) {
    let our_node_id = endpoint.node_id();

    // ([u8;32], Vec<NodeId>, Option<relay_url_str>)
    let entries: Vec<([u8; 32], Vec<iroh::NodeId>, Option<String>)> = {
        let db = db.lock().await;
        let conn = db.conn();
        let mut stmt = match conn.prepare(
            "SELECT c.content_hash, c.publisher_node_id, c.publisher_relay_url
             FROM seeding s
             JOIN content c ON s.content_id = c.content_id
             WHERE s.active = 1 AND c.content_hash IS NOT NULL",
        ) {
            Ok(s) => s,
            Err(e) => {
                warn!("Failed to query active seeding for resume: {e}");
                return;
            }
        };

        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        });

        let mut entries = Vec::new();
        if let Ok(rows) = rows {
            for row in rows.flatten() {
                let (hash_hex, node_id_opt, relay_url_opt) = row;
                let hex_str = hash_hex.strip_prefix("0x").unwrap_or(&hash_hex);
                if let Ok(bytes) = alloy::hex::decode(hex_str) {
                    if bytes.len() == 32 {
                        let mut hash = [0u8; 32];
                        hash.copy_from_slice(&bytes);

                        // Start with the publisher's NodeId as bootstrap — filter self to avoid
                        // the "Connecting to ourself is not supported" warning when the publisher
                        // is this node (e.g. seeding content we published ourselves).
                        let mut bootstrap: Vec<iroh::NodeId> = node_id_opt
                            .filter(|s| !s.is_empty())
                            .and_then(|s| s.parse::<iroh::NodeId>().ok())
                            .filter(|&id| id != our_node_id)
                            .into_iter()
                            .collect();

                        // Also include previously discovered seeders from the DB.
                        // These survive restarts, enabling reconnection even when the
                        // publisher's NodeId is our own or is unreachable.
                        let stored_seeders: Vec<iroh::NodeId> = {
                            let mut seeder_stmt = conn
                                .prepare("SELECT node_id FROM content_seeders WHERE content_hash = ?1")
                                .unwrap();
                            let seeder_rows = seeder_stmt
                                .query_map(rusqlite::params![format!("0x{}", alloy::hex::encode(hash))], |r| {
                                    r.get::<_, String>(0)
                                })
                                .unwrap();
                            seeder_rows
                                .flatten()
                                .filter_map(|s| s.parse::<iroh::NodeId>().ok())
                                .filter(|&id| id != our_node_id)
                                .collect()
                        };
                        for id in stored_seeders {
                            if !bootstrap.contains(&id) {
                                bootstrap.push(id);
                            }
                        }

                        entries.push((hash, bootstrap, relay_url_opt.filter(|s| !s.is_empty())));
                    }
                }
            }
        }
        entries
    };

    if entries.is_empty() {
        return;
    }

    info!("Resuming gossip announcements for {} actively seeded items", entries.len());
    for (content_hash, bootstrap, relay_url_opt) in entries {
        // Pre-populate endpoint routing table with relay URL so gossip can dial bootstrap peers.
        for &node_id in &bootstrap {
            let mut addr = iroh::NodeAddr::from(node_id);
            if let Some(relay_url_str) = relay_url_opt.as_deref() {
                if let Ok(relay_url) = relay_url_str.parse() {
                    addr = addr.with_relay_url(relay_url);
                }
            }
            let _ = endpoint.add_node_addr(addr);
        }

        if let Err(e) = gossip_tx
            .send(GossipCmd::AnnounceSeeding {
                content_hash,
                bootstrap,
            })
            .await
        {
            warn!("Failed to resume seeding announcement: {e}");
        }
    }
}
