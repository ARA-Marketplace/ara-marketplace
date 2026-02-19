//! Background gossip actor for seeder announcements.
//!
//! `GossipReceiver` from iroh-gossip is `!Sync`, which means it can't be stored
//! in AppState behind `Arc<Mutex<>>` (Tauri requires Send futures). This actor
//! confines all `!Sync` types to spawned background tasks, exposing only a
//! `mpsc::Sender<GossipCmd>` (which IS Send+Sync) for Tauri commands to use.

use std::collections::HashMap;

use ara_p2p::content::ContentHash;
use ara_p2p::discovery::{topic_for_content, GossipMessage};
use bytes::Bytes;
use iroh::NodeId;
use iroh_gossip::net::{Gossip, GossipSender};
use tokio::sync::mpsc;
use tracing::{info, warn};

/// Commands sent from Tauri commands to the gossip actor.
pub enum GossipCmd {
    /// Join the gossip topic for this content and broadcast a SeederAnnounce.
    AnnounceSeeding { content_hash: ContentHash },
    /// Broadcast a SeederLeave and drop the topic subscription.
    LeaveSeeding { content_hash: ContentHash },
}

struct GossipActor {
    gossip: Gossip,
    node_id: NodeId,
    rx: mpsc::Receiver<GossipCmd>,
    active_topics: HashMap<ContentHash, GossipSender>,
}

impl GossipActor {
    async fn run(mut self) {
        info!("Gossip actor started");
        while let Some(cmd) = self.rx.recv().await {
            match cmd {
                GossipCmd::AnnounceSeeding { content_hash } => {
                    if let Err(e) = self.handle_announce(content_hash).await {
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

    async fn handle_announce(&mut self, content_hash: ContentHash) -> anyhow::Result<()> {
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
        info!("Joining gossip topic for {} (topic: {})", hash_hex, topic_id);

        let bootstrap: Vec<NodeId> = vec![];
        let topic = self.gossip.join(topic_id, bootstrap).await?;
        let (sender, _receiver) = topic.split();
        // We drop the receiver — we only need the sender for broadcasting.
        // The sender keeps us in the gossip topic.
        // Incoming message handling (for peer discovery) will be added when
        // cross-node download is implemented.

        // Broadcast our seeder announcement
        sender.broadcast(encoded).await?;

        info!("Announced seeding for {}", hash_hex);
        self.active_topics.insert(content_hash, sender);
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

        Ok(())
    }
}

/// Spawn the gossip actor as a background task.
/// Returns an `mpsc::Sender` for sending commands (Send+Sync safe).
pub fn spawn(gossip: Gossip, node_id: NodeId) -> mpsc::Sender<GossipCmd> {
    let (tx, rx) = mpsc::channel(64);

    let actor = GossipActor {
        gossip,
        node_id,
        rx,
        active_topics: HashMap::new(),
    };

    tokio::spawn(actor.run());

    tx
}
