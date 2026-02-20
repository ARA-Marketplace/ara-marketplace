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
use iroh_gossip::net::{Event, GossipEvent, Gossip, GossipReceiver, GossipSender};
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

/// Known seeders discovered via gossip, keyed by content hash.
/// Shared with the rest of the app (Send+Sync via Arc<Mutex>).
pub type KnownSeeders = Arc<Mutex<HashMap<ContentHash, HashSet<NodeId>>>>;

struct GossipActor {
    gossip: Gossip,
    node_id: NodeId,
    rx: mpsc::Receiver<GossipCmd>,
    active_topics: HashMap<ContentHash, GossipSender>,
    known_seeders: KnownSeeders,
}

impl GossipActor {
    async fn run(mut self) {
        info!("Gossip actor started");
        while let Some(cmd) = self.rx.recv().await {
            match cmd {
                GossipCmd::AnnounceSeeding { content_hash, bootstrap } => {
                    if let Err(e) = self.handle_announce(content_hash, bootstrap).await {
                        warn!("Gossip announce failed: {e}");
                    }
                }
                GossipCmd::LeaveSeeding { content_hash } => {
                    if let Err(e) = self.handle_leave(content_hash).await {
                        warn!("Gossip leave failed: {e}");
                    }
                }
            }
        }
        info!("Gossip actor stopped (channel closed)");
    }

    fn encode_msg(&self, msg: &GossipMessage) -> anyhow::Result<Bytes> {
        Ok(Bytes::from(serde_json::to_vec(msg)?))
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

        let topic = self.gossip.join(topic_id, bootstrap).await?;
        let (sender, receiver) = topic.split();

        // Spawn a background task to listen for incoming gossip messages
        let known_seeders = self.known_seeders.clone();
        let our_node_id = self.node_id;
        tokio::spawn(Self::recv_loop(receiver, known_seeders, our_node_id));

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
    ) {
        use futures_lite::StreamExt;
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
                            }
                        }
                        Err(e) => {
                            warn!("Failed to parse gossip message: {e}");
                        }
                    }
                }
                Ok(_) => {} // Joined/NeighborUp/NeighborDown — ignore for now
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
) -> mpsc::Sender<GossipCmd> {
    let (tx, rx) = mpsc::channel(64);

    let actor = GossipActor {
        gossip,
        node_id,
        rx,
        active_topics: HashMap::new(),
        known_seeders,
    };

    tokio::spawn(actor.run());

    tx
}
