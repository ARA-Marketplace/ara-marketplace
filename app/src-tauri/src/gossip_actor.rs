//! Background gossip actor for seeder announcements and peer discovery.
//!
//! `GossipReceiver` from iroh-gossip is `!Sync`, which means it can't be stored
//! in AppState behind `Arc<Mutex<>>` (Tauri requires Send futures). This actor
//! confines all `!Sync` types to spawned background tasks, exposing only an
//! `mpsc::Sender<GossipCmd>` (which IS Send+Sync) for Tauri commands to use.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

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
}

/// Internal events sent from per-topic recv_loop tasks back to the actor.
enum RecvEvent {
    /// A new neighbor joined the gossip overlay for this content.
    /// The actor should re-broadcast our SeederAnnounce so they discover us.
    NeighborUp { content_hash: ContentHash },
    /// A seeder was discovered or left — UI stats may have changed.
    PeerChanged,
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

    /// Re-broadcast our SeederAnnounce when a new neighbor joins the topic.
    async fn handle_neighbor_up(&self, content_hash: ContentHash) -> anyhow::Result<()> {
        let hash_hex = alloy::hex::encode(content_hash);

        if let Some(sender) = self.active_topics.get(&content_hash) {
            let msg = GossipMessage::SeederAnnounce {
                content_hash,
                node_id_bytes: *self.node_id.as_bytes(),
            };
            let encoded = self.encode_msg(&msg)?;
            info!("Re-announcing seeding on NeighborUp for {}", hash_hex);
            sender.broadcast(encoded).await?;
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
                                    seeders
                                        .entry(content_hash)
                                        .or_default()
                                        .insert(peer_id);
                                    drop(seeders);
                                    let _ = event_tx.send(RecvEvent::PeerChanged);
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
                    {
                        let mut seeders = known_seeders.lock().await;
                        seeders.entry(content_hash).or_default().insert(peer_id);
                    }
                    let _ = event_tx.send(RecvEvent::NeighborUp { content_hash });
                    let _ = event_tx.send(RecvEvent::PeerChanged);
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
/// Discovered seeders are written into the shared `known_seeders` map.
pub fn spawn(
    gossip: Gossip,
    node_id: NodeId,
    known_seeders: KnownSeeders,
    app_handle: tauri::AppHandle,
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
    };

    tokio::spawn(actor.run());

    tx
}
