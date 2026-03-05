//! Background gossip actor for seeder announcements and peer discovery.
//!
//! `GossipReceiver` from iroh-gossip is `!Sync`, which means it can't be stored
//! in AppState behind `Arc<Mutex<>>` (Tauri requires Send futures). This actor
//! confines all `!Sync` types to spawned background tasks, exposing only an
//! `mpsc::Sender<GossipCmd>` (which IS Send+Sync) for Tauri commands to use.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use ara_core::storage::Database;
use ara_p2p::content::ContentHash;
use ara_p2p::discovery::{topic_for_content, GossipMessage};
use bytes::Bytes;
use iroh::NodeId;
use iroh_gossip::net::{Event, GossipEvent, Gossip, GossipReceiver, GossipSender, JoinOptions};
use tauri::Emitter;
use tokio::sync::{mpsc, Mutex};
use tracing::{info, warn};

/// Commands sent from Tauri commands to the gossip actor.
pub enum GossipCmd {
    /// Join the gossip topic for this content and broadcast a SeederAnnounce.
    /// `bootstrap` should contain known peers (e.g. the publisher's NodeId) to
    /// seed the gossip overlay; without at least one bootstrap peer, nodes on
    /// the same topic cannot discover each other.
    AnnounceSeeding {
        content_hash: ContentHash,
        bootstrap: Vec<NodeId>,
    },
    /// Broadcast a SeederLeave and drop the topic subscription.
    LeaveSeeding { content_hash: ContentHash },
    /// Broadcast a buyer-signed delivery receipt on the gossip topic for the given content.
    /// The receipt proves the buyer received the content from a specific seeder.
    BroadcastDeliveryReceipt {
        content_hash: ContentHash,
        content_id: [u8; 32],
        seeder_eth_address: [u8; 20],
        buyer_eth_address: [u8; 20],
        signature: Vec<u8>,
        timestamp: u64,
        bytes_served: u64,
    },
    /// Broadcast this node's Ethereum address on all active seeding topics.
    /// Lets creators map iroh NodeId → ETH address for reward distribution.
    BroadcastSeederIdentity {
        node_id: [u8; 32],
        eth_address: [u8; 20],
        /// Ed25519 signature: sign(keccak256("AraSeeder:" || node_id || eth_address))
        signature: Vec<u8>,
    },
}

/// Internal events sent from per-topic recv_loop tasks back to the actor.
enum RecvEvent {
    /// A new neighbor joined the gossip overlay for this content.
    /// The actor should re-broadcast our SeederAnnounce so they discover us.
    NeighborUp { content_hash: ContentHash },
    /// A seeder was discovered or left — UI stats may have changed.
    PeerChanged,
    /// A new seeder NodeId was discovered and should be persisted to the DB
    /// so it can be used as a bootstrap peer after restart.
    SeederPersist { content_hash: ContentHash, node_id: NodeId },
    /// A delivery receipt was received from the gossip overlay — store it in the DB.
    DeliveryReceiptReceived {
        content_id_hex: String,
        seeder_eth_address_hex: String,
        buyer_eth_address_hex: String,
        signature_hex: String,
        timestamp: i64,
        bytes_served: u64,
    },
    /// A seeder identity mapping was received — update the DB with NodeId → ETH address.
    SeederIdentityReceived {
        content_hash: ContentHash,
        node_id: NodeId,
        eth_address_hex: String,
    },
    /// Content was flagged via moderation — update local DB.
    ContentFlagged {
        content_id_hex: String,
        reason: u8,
        is_emergency: bool,
    },
    /// Content was purged via moderation consensus — delete blob and stop seeding.
    ContentPurged {
        content_id_hex: String,
        content_hash: ContentHash,
    },
}

/// Known seeders discovered via gossip, keyed by content hash.
/// Shared with the rest of the app (Send+Sync via Arc<Mutex>).
pub type KnownSeeders = Arc<Mutex<HashMap<ContentHash, HashSet<NodeId>>>>;

struct GossipActor {
    gossip: Gossip,
    node_id: NodeId,
    rx: mpsc::Receiver<GossipCmd>,
    event_rx: mpsc::UnboundedReceiver<RecvEvent>,
    event_tx: mpsc::UnboundedSender<RecvEvent>,
    active_topics: HashMap<ContentHash, GossipSender>,
    known_seeders: KnownSeeders,
    app_handle: tauri::AppHandle,
    db: Arc<Mutex<Database>>,
}

impl GossipActor {
    async fn run(mut self) {
        info!("Gossip actor started");
        // Fire first re-announce 60 s after start, then every 60 s.
        // Serves as a heartbeat: peers that miss the initial broadcast will
        // receive the next periodic announcement once gossip bootstrap succeeds.
        let mut reannounce_interval = tokio::time::interval_at(
            tokio::time::Instant::now() + tokio::time::Duration::from_secs(60),
            tokio::time::Duration::from_secs(60),
        );
        loop {
            tokio::select! {
                cmd = self.rx.recv() => {
                    match cmd {
                        Some(GossipCmd::AnnounceSeeding { content_hash, bootstrap }) => {
                            if let Err(e) = self.handle_announce(content_hash, bootstrap).await {
                                warn!("Gossip announce failed: {e}");
                            }
                        }
                        Some(GossipCmd::LeaveSeeding { content_hash }) => {
                            if let Err(e) = self.handle_leave(content_hash).await {
                                warn!("Gossip leave failed: {e}");
                            }
                        }
                        Some(GossipCmd::BroadcastDeliveryReceipt {
                            content_hash, content_id, seeder_eth_address,
                            buyer_eth_address, signature, timestamp, bytes_served,
                        }) => {
                            if let Err(e) = self.handle_broadcast_receipt(
                                content_hash, content_id, seeder_eth_address,
                                buyer_eth_address, signature, timestamp, bytes_served,
                            ).await {
                                warn!("Broadcast delivery receipt failed: {e}");
                            }
                        }
                        Some(GossipCmd::BroadcastSeederIdentity { node_id, eth_address, signature }) => {
                            if let Err(e) = self.handle_broadcast_identity(node_id, eth_address, signature).await {
                                warn!("Broadcast seeder identity failed: {e}");
                            }
                        }
                        None => break, // command channel closed
                    }
                }
                evt = self.event_rx.recv() => {
                    match evt {
                        Some(RecvEvent::NeighborUp { content_hash }) => {
                            if let Err(e) = self.handle_neighbor_up(content_hash).await {
                                warn!("Re-announce on NeighborUp failed: {e}");
                            }
                        }
                        Some(RecvEvent::PeerChanged) => {
                            let _ = self.app_handle.emit("seeder-stats-updated", ());
                        }
                        Some(RecvEvent::SeederPersist { content_hash, node_id }) => {
                            let hash_hex = format!("0x{}", alloy::hex::encode(content_hash));
                            let now = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs() as i64;
                            let db = self.db.lock().await;
                            let _ = db.conn().execute(
                                "INSERT OR IGNORE INTO content_seeders (content_hash, node_id, discovered_at) VALUES (?1, ?2, ?3)",
                                rusqlite::params![hash_hex, node_id.to_string(), now],
                            );
                        }
                        Some(RecvEvent::DeliveryReceiptReceived {
                            content_id_hex, seeder_eth_address_hex,
                            buyer_eth_address_hex, signature_hex, timestamp, bytes_served,
                        }) => {
                            let db = self.db.lock().await;
                            if let Err(e) = db.insert_delivery_receipt(
                                &content_id_hex,
                                &seeder_eth_address_hex,
                                &buyer_eth_address_hex,
                                &signature_hex,
                                timestamp,
                                bytes_served,
                            ) {
                                warn!("Failed to store delivery receipt: {e}");
                            }
                            // Credit bytes_served to our seeding entry (if we're seeding this content).
                            // This is a fallback for cases where the iroh blob event handler missed
                            // the transfer (e.g. timing race or internal blob metadata transfer).
                            if bytes_served > 0 {
                                let _ = db.conn().execute(
                                    "UPDATE seeding SET bytes_served = MAX(bytes_served, ?1)
                                     WHERE content_id = ?2",
                                    rusqlite::params![bytes_served as i64, &content_id_hex],
                                );
                            }
                        }
                        Some(RecvEvent::SeederIdentityReceived { content_hash, node_id, eth_address_hex }) => {
                            let hash_hex = format!("0x{}", alloy::hex::encode(content_hash));
                            let db = self.db.lock().await;
                            if let Err(e) = db.set_seeder_eth_address(
                                &hash_hex,
                                &node_id.to_string(),
                                &eth_address_hex,
                            ) {
                                warn!("Failed to store seeder identity: {e}");
                            }
                        }
                        Some(RecvEvent::ContentFlagged { content_id_hex, reason, is_emergency }) => {
                            info!("Processing content flag for {} (reason={}, emergency={})", content_id_hex, reason, is_emergency);
                            let status = if is_emergency { "emergency_flagged" } else { "flagged" };
                            let db = self.db.lock().await;
                            let _ = db.conn().execute(
                                "UPDATE content SET moderation_status = ?1 WHERE content_id = ?2",
                                rusqlite::params![status, &content_id_hex],
                            );
                        }
                        Some(RecvEvent::ContentPurged { content_id_hex, content_hash: _ }) => {
                            info!("Processing content purge for {}", content_id_hex);
                            let db = self.db.lock().await;
                            let _ = db.conn().execute(
                                "UPDATE content SET moderation_status = 'purged', active = 0 WHERE content_id = ?1",
                                rusqlite::params![&content_id_hex],
                            );
                            let _ = db.conn().execute(
                                "UPDATE seeding SET active = 0 WHERE content_id = ?1",
                                rusqlite::params![&content_id_hex],
                            );
                        }
                        None => {} // all recv_loop senders dropped — fine
                    }
                }
                _ = reannounce_interval.tick() => {
                    self.periodic_reannounce().await;
                }
            }
        }
        info!("Gossip actor stopped (channel closed)");
    }

    /// Periodically re-broadcast SeederAnnounce to all active topics.
    /// This acts as a heartbeat so peers that missed the initial broadcast
    /// (e.g. because gossip bootstrap was still connecting) will eventually
    /// discover us once the gossip overlay is established.
    async fn periodic_reannounce(&self) {
        let hashes: Vec<ContentHash> = self.active_topics.keys().cloned().collect();
        if hashes.is_empty() {
            return;
        }
        info!("Periodic re-announce for {} active gossip topics", hashes.len());
        for content_hash in hashes {
            if let Some(sender) = self.active_topics.get(&content_hash) {
                let msg = GossipMessage::SeederAnnounce {
                    content_hash,
                    node_id_bytes: *self.node_id.as_bytes(),
                };
                if let Ok(encoded) = self.encode_msg(&msg) {
                    if let Err(e) = sender.broadcast(encoded).await {
                        warn!("Periodic re-announce failed for {}: {e}", alloy::hex::encode(content_hash));
                    }
                }
            }
        }
    }

    fn encode_msg(&self, msg: &GossipMessage) -> anyhow::Result<Bytes> {
        Ok(Bytes::from(serde_json::to_vec(msg)?))
    }

    /// Re-broadcast our SeederAnnounce and any pending delivery receipts
    /// when a new neighbor joins the topic.  Receipt re-broadcast fixes the
    /// race where the buyer broadcasts a receipt before the gossip bootstrap
    /// connection to the publisher is established.
    async fn handle_neighbor_up(&self, content_hash: ContentHash) -> anyhow::Result<()> {
        let hash_hex = alloy::hex::encode(content_hash);

        let Some(sender) = self.active_topics.get(&content_hash) else {
            return Ok(());
        };

        // Re-announce ourselves as a seeder
        let msg = GossipMessage::SeederAnnounce {
            content_hash,
            node_id_bytes: *self.node_id.as_bytes(),
        };
        let encoded = self.encode_msg(&msg)?;
        info!("Re-announcing seeding on NeighborUp for {}", hash_hex);
        sender.broadcast(encoded).await?;

        // Re-broadcast delivery receipts stored in the DB for this content.
        // Look up content_id from content_hash, then find any receipts.
        let hash_hex_prefixed = format!("0x{}", hash_hex);
        let receipts: Vec<(String, String, String, String, i64, u64)> = {
            let db = self.db.lock().await;
            // content_hash → content_id → delivery_receipts
            let content_id: Option<String> = db.conn().query_row(
                "SELECT content_id FROM content WHERE content_hash = ?1 LIMIT 1",
                rusqlite::params![&hash_hex_prefixed],
                |row| row.get(0),
            ).ok();
            if let Some(cid) = content_id {
                let mut stmt = db.conn().prepare(
                    "SELECT content_id, seeder_eth_address, buyer_eth_address, signature, timestamp, bytes_served
                     FROM delivery_receipts WHERE content_id = ?1"
                )?;
                let rows: Vec<_> = stmt.query_map(rusqlite::params![&cid], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, i64>(5)? as u64,
                    ))
                })?
                .filter_map(|r| r.ok())
                .collect();
                rows
            } else {
                vec![]
            }
        };

        if !receipts.is_empty() {
            info!("Re-broadcasting {} delivery receipt(s) on NeighborUp for {}", receipts.len(), hash_hex);
        }
        for (content_id_hex, seeder_hex, buyer_hex, sig_hex, ts, bytes_served) in receipts {
            // Parse hex strings back to byte arrays for the gossip message
            let content_id = parse_hex_32(content_id_hex.strip_prefix("0x").unwrap_or(&content_id_hex));
            let seeder = parse_hex_20(seeder_hex.strip_prefix("0x").unwrap_or(&seeder_hex));
            let buyer = parse_hex_20(buyer_hex.strip_prefix("0x").unwrap_or(&buyer_hex));
            let sig = alloy::hex::decode(sig_hex.strip_prefix("0x").unwrap_or(&sig_hex)).unwrap_or_default();

            if let (Some(cid), Some(s), Some(b)) = (content_id, seeder, buyer) {
                let msg = GossipMessage::DeliveryReceipt {
                    content_id: cid,
                    seeder_eth_address: s,
                    buyer_eth_address: b,
                    signature: sig,
                    timestamp: ts as u64,
                    bytes_served,
                };
                if let Ok(encoded) = self.encode_msg(&msg) {
                    let _ = sender.broadcast(encoded).await;
                }
            }
        }

        Ok(())
    }

    async fn handle_announce(&mut self, content_hash: ContentHash, bootstrap: Vec<NodeId>) -> anyhow::Result<()> {
        let hash_hex = alloy::hex::encode(content_hash);

        let msg = GossipMessage::SeederAnnounce {
            content_hash,
            node_id_bytes: *self.node_id.as_bytes(),
        };
        let encoded = self.encode_msg(&msg)?;

        // If already subscribed, just re-broadcast
        if let Some(sender) = self.active_topics.get(&content_hash) {
            info!("Re-announcing seeding for {}", hash_hex);
            sender.broadcast(encoded).await?;
            return Ok(());
        }

        // Join the gossip topic for this content
        let topic_id = topic_for_content(&content_hash);
        info!("Joining gossip topic for {} (topic: {}, bootstrap peers: {})", hash_hex, topic_id, bootstrap.len());

        let topic = self.gossip.join_with_opts(topic_id, JoinOptions::with_bootstrap(bootstrap));
        let (sender, receiver) = topic.split();

        // Spawn a background task to listen for incoming gossip messages
        let known_seeders = self.known_seeders.clone();
        let our_node_id = self.node_id;
        let event_tx = self.event_tx.clone();
        tokio::spawn(Self::recv_loop(receiver, known_seeders, our_node_id, content_hash, event_tx));

        // Broadcast our seeder announcement
        sender.broadcast(encoded).await?;

        info!("Announced seeding for {}", hash_hex);
        self.active_topics.insert(content_hash, sender);
        Ok(())
    }

    /// Background task that listens for incoming gossip messages on a topic.
    async fn recv_loop(
        mut receiver: GossipReceiver,
        known_seeders: KnownSeeders,
        our_node_id: NodeId,
        content_hash: ContentHash,
        event_tx: mpsc::UnboundedSender<RecvEvent>,
    ) {
        use futures_lite::StreamExt;
        let hash_hex = alloy::hex::encode(content_hash);
        while let Some(event) = receiver.next().await {
            match event {
                Ok(Event::Gossip(GossipEvent::Received(msg))) => {
                    match serde_json::from_slice::<GossipMessage>(&msg.content) {
                        Ok(GossipMessage::SeederAnnounce {
                            content_hash,
                            node_id_bytes,
                        }) => {
                            let peer_id = NodeId::from_bytes(&node_id_bytes);
                            if let Ok(peer_id) = peer_id {
                                if peer_id != our_node_id {
                                    info!(
                                        "Discovered seeder {} for content {}",
                                        peer_id,
                                        alloy::hex::encode(content_hash)
                                    );
                                    let mut seeders = known_seeders.lock().await;
                                    let is_new = seeders
                                        .entry(content_hash)
                                        .or_default()
                                        .insert(peer_id);
                                    drop(seeders);
                                    let _ = event_tx.send(RecvEvent::PeerChanged);
                                    // Persist new seeders so they survive restart
                                    if is_new {
                                        let _ = event_tx.send(RecvEvent::SeederPersist { content_hash, node_id: peer_id });
                                    }
                                }
                            }
                        }
                        Ok(GossipMessage::SeederLeave {
                            content_hash,
                            node_id_bytes,
                        }) => {
                            let peer_id = NodeId::from_bytes(&node_id_bytes);
                            if let Ok(peer_id) = peer_id {
                                info!(
                                    "Seeder {} left for content {}",
                                    peer_id,
                                    alloy::hex::encode(content_hash)
                                );
                                let mut seeders = known_seeders.lock().await;
                                if let Some(set) = seeders.get_mut(&content_hash) {
                                    set.remove(&peer_id);
                                }
                                drop(seeders);
                                let _ = event_tx.send(RecvEvent::PeerChanged);
                            }
                        }
                        Ok(GossipMessage::DeliveryReceipt {
                            content_id,
                            seeder_eth_address,
                            buyer_eth_address,
                            signature,
                            timestamp,
                            bytes_served,
                        }) => {
                            // Store receipt — signature is verified on-chain when seeder claims rewards
                            let content_id_hex = format!("0x{}", alloy::hex::encode(content_id));
                            let seeder_hex = alloy::primitives::Address::from(seeder_eth_address).to_checksum(None);
                            let buyer_hex = alloy::primitives::Address::from(buyer_eth_address).to_checksum(None);
                            let sig_hex = format!("0x{}", alloy::hex::encode(signature));
                            let _ = event_tx.send(RecvEvent::DeliveryReceiptReceived {
                                content_id_hex,
                                seeder_eth_address_hex: seeder_hex,
                                buyer_eth_address_hex: buyer_hex,
                                signature_hex: sig_hex,
                                timestamp: timestamp as i64,
                                bytes_served,
                            });
                        }
                        Ok(GossipMessage::SeederIdentity { node_id, eth_address, .. }) => {
                            let peer_id = NodeId::from_bytes(&node_id);
                            if let Ok(peer_id) = peer_id {
                                let eth_hex = alloy::primitives::Address::from(eth_address).to_checksum(None);
                                let _ = event_tx.send(RecvEvent::SeederIdentityReceived {
                                    content_hash,
                                    node_id: peer_id,
                                    eth_address_hex: eth_hex,
                                });
                            }
                        }
                        Ok(GossipMessage::ContentFlagged {
                            content_id,
                            reason,
                            is_emergency,
                            ..
                        }) => {
                            let content_id_hex = format!("0x{}", alloy::hex::encode(content_id));
                            info!(
                                "Content flagged via gossip: {} (reason={}, emergency={})",
                                content_id_hex, reason, is_emergency
                            );
                            let _ = event_tx.send(RecvEvent::ContentFlagged {
                                content_id_hex,
                                reason,
                                is_emergency,
                            });
                        }
                        Ok(GossipMessage::ContentPurge {
                            content_id,
                            content_hash: purge_hash,
                            ..
                        }) => {
                            let content_id_hex = format!("0x{}", alloy::hex::encode(content_id));
                            info!(
                                "Content purge received via gossip: {}",
                                content_id_hex
                            );
                            let _ = event_tx.send(RecvEvent::ContentPurged {
                                content_id_hex,
                                content_hash: purge_hash,
                            });
                        }
                        Err(e) => {
                            warn!("Failed to parse gossip message: {e}");
                        }
                    }
                }
                Ok(Event::Gossip(GossipEvent::NeighborUp(peer_id))) => {
                    info!("Neighbor up: {} on topic {}", peer_id, hash_hex);
                    // Add to known_seeders immediately on gossip connection —
                    // don't wait for a SeederAnnounce message, which can be
                    // missed if it was broadcast before bootstrap connected.
                    let is_new = {
                        let mut seeders = known_seeders.lock().await;
                        seeders.entry(content_hash).or_default().insert(peer_id)
                    };
                    let _ = event_tx.send(RecvEvent::NeighborUp { content_hash });
                    let _ = event_tx.send(RecvEvent::PeerChanged);
                    // Persist so this peer can be used as bootstrap after restart
                    if is_new {
                        let _ = event_tx.send(RecvEvent::SeederPersist { content_hash, node_id: peer_id });
                    }
                }
                Ok(Event::Gossip(GossipEvent::NeighborDown(peer_id))) => {
                    info!("Neighbor down: {} on topic {}", peer_id, hash_hex);
                    // Remove from known_seeders when the gossip connection drops.
                    {
                        let mut seeders = known_seeders.lock().await;
                        if let Some(set) = seeders.get_mut(&content_hash) {
                            set.remove(&peer_id);
                        }
                    }
                    let _ = event_tx.send(RecvEvent::PeerChanged);
                }
                Ok(Event::Gossip(GossipEvent::Joined(peers))) => {
                    info!("Joined gossip topic {} with {} initial peers", hash_hex, peers.len());
                    {
                        let mut seeders = known_seeders.lock().await;
                        for peer_id in &peers {
                            if *peer_id != our_node_id {
                                seeders.entry(content_hash).or_default().insert(*peer_id);
                            }
                        }
                    }
                    if !peers.is_empty() {
                        let _ = event_tx.send(RecvEvent::PeerChanged);
                        // Re-announce so the newly connected peer discovers us immediately
                        let _ = event_tx.send(RecvEvent::NeighborUp { content_hash });
                    }
                }
                Ok(_) => {} // Other events — ignore
                Err(e) => {
                    warn!("Gossip receive error: {e}");
                    break;
                }
            }
        }
    }

    async fn handle_broadcast_receipt(
        &mut self,
        content_hash: ContentHash,
        content_id: [u8; 32],
        seeder_eth_address: [u8; 20],
        buyer_eth_address: [u8; 20],
        signature: Vec<u8>,
        timestamp: u64,
        bytes_served: u64,
    ) -> anyhow::Result<()> {
        let hash_hex = alloy::hex::encode(content_hash);

        // Auto-join the gossip topic if not already subscribed.
        // This happens when a buyer broadcasts a receipt before starting to seed —
        // they've downloaded the content but haven't joined the gossip topic yet.
        if !self.active_topics.contains_key(&content_hash) {
            info!("Auto-joining gossip topic for receipt broadcast on {}", hash_hex);

            // Look up known seeders from DB to use as bootstrap peers
            let bootstrap: Vec<NodeId> = {
                let db = self.db.lock().await;
                let hash_hex_prefixed = format!("0x{}", hash_hex);
                let mut stmt = db.conn().prepare(
                    "SELECT node_id FROM content_seeders WHERE content_hash = ?1"
                ).unwrap_or_else(|_| db.conn().prepare("SELECT '' WHERE 0").unwrap());
                stmt.query_map(rusqlite::params![&hash_hex_prefixed], |row| {
                    row.get::<_, String>(0)
                })
                .ok()
                .into_iter()
                .flatten()
                .filter_map(|r| r.ok())
                .filter_map(|s| s.parse::<NodeId>().ok())
                .filter(|id| *id != self.node_id)
                .collect()
            };

            info!("Bootstrapping receipt topic {} with {} peers", hash_hex, bootstrap.len());
            let topic_id = topic_for_content(&content_hash);
            let topic = self.gossip.join_with_opts(topic_id, JoinOptions::with_bootstrap(bootstrap));
            let (sender, receiver) = topic.split();

            let known_seeders = self.known_seeders.clone();
            let our_node_id = self.node_id;
            let event_tx = self.event_tx.clone();
            tokio::spawn(Self::recv_loop(receiver, known_seeders, our_node_id, content_hash, event_tx));

            self.active_topics.insert(content_hash, sender);
        }

        let msg = GossipMessage::DeliveryReceipt {
            content_id,
            seeder_eth_address,
            buyer_eth_address,
            signature,
            timestamp,
            bytes_served,
        };
        let sender = self.active_topics.get(&content_hash).unwrap();
        let encoded = self.encode_msg(&msg)?;
        sender.broadcast(encoded).await?;
        info!("Broadcast delivery receipt for content {}", hash_hex);
        Ok(())
    }

    async fn handle_broadcast_identity(
        &self,
        node_id: [u8; 32],
        eth_address: [u8; 20],
        signature: Vec<u8>,
    ) -> anyhow::Result<()> {
        let msg = GossipMessage::SeederIdentity { node_id, eth_address, signature };
        let encoded = self.encode_msg(&msg)?;
        for (hash, sender) in &self.active_topics {
            if let Err(e) = sender.broadcast(encoded.clone()).await {
                warn!("SeederIdentity broadcast failed for {}: {e}", alloy::hex::encode(hash));
            }
        }
        info!("Broadcast SeederIdentity on {} topics", self.active_topics.len());
        Ok(())
    }

    async fn handle_leave(&mut self, content_hash: ContentHash) -> anyhow::Result<()> {
        let hash_hex = alloy::hex::encode(content_hash);

        if let Some(sender) = self.active_topics.get(&content_hash) {
            // Broadcast leave message before dropping
            let msg = GossipMessage::SeederLeave {
                content_hash,
                node_id_bytes: *self.node_id.as_bytes(),
            };
            if let Ok(encoded) = self.encode_msg(&msg) {
                let _ = sender.broadcast(encoded).await;
            }
        }

        // Remove sender — dropping it leaves the gossip topic
        if self.active_topics.remove(&content_hash).is_some() {
            info!("Left gossip topic for {}", hash_hex);
        }

        // Clean up known seeders for this content
        let mut seeders = self.known_seeders.lock().await;
        seeders.remove(&content_hash);

        Ok(())
    }
}

/// Spawn the gossip actor as a background task.
/// Returns an `mpsc::Sender` for sending commands (Send+Sync safe).
/// Discovered seeders are written into the shared `known_seeders` map and
/// persisted to the DB so they survive restarts.
pub fn spawn(
    gossip: Gossip,
    node_id: NodeId,
    known_seeders: KnownSeeders,
    app_handle: tauri::AppHandle,
    db: Arc<Mutex<Database>>,
) -> mpsc::Sender<GossipCmd> {
    let (tx, rx) = mpsc::channel(64);
    let (event_tx, event_rx) = mpsc::unbounded_channel();

    let actor = GossipActor {
        gossip,
        node_id,
        rx,
        event_rx,
        event_tx,
        active_topics: HashMap::new(),
        known_seeders,
        app_handle,
        db,
    };

    tokio::spawn(actor.run());

    tx
}

/// Parse a hex string to a [u8; 32], returning None on failure.
fn parse_hex_32(hex: &str) -> Option<[u8; 32]> {
    let bytes = alloy::hex::decode(hex).ok()?;
    bytes.try_into().ok()
}

/// Parse a hex string to a [u8; 20], returning None on failure.
fn parse_hex_20(hex: &str) -> Option<[u8; 20]> {
    let bytes = alloy::hex::decode(hex).ok()?;
    bytes.try_into().ok()
}
