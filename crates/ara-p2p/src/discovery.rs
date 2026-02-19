use std::collections::HashMap;

use anyhow::Result;
use bytes::Bytes;
use iroh::NodeId;
use iroh_gossip::net::{GossipReceiver, GossipSender, Gossip};
use iroh_gossip::proto::TopicId;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::content::ContentHash;

/// Message types broadcast over gossip topics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GossipMessage {
    /// Announce that this node is available to seed content.
    SeederAnnounce {
        content_hash: ContentHash,
        node_id_bytes: [u8; 32],
    },
    /// Announce that this node is no longer seeding content.
    SeederLeave {
        content_hash: ContentHash,
        node_id_bytes: [u8; 32],
    },
}

/// Derive a gossip topic from a content hash.
/// Each piece of content has its own gossip topic for seeder discovery.
pub fn topic_for_content(hash: &ContentHash) -> TopicId {
    // Use the content hash directly as the topic ID
    TopicId::from(*hash)
}

/// A well-known global topic for new content announcements.
pub fn global_announce_topic() -> TopicId {
    // Fixed topic ID for the global "new content" feed
    let mut bytes = [0u8; 32];
    bytes[..11].copy_from_slice(b"ara-global\0");
    TopicId::from(bytes)
}

/// Manages a single gossip topic subscription (one per content being seeded/discovered).
pub struct TopicHandle {
    pub topic_id: TopicId,
    pub sender: GossipSender,
    pub receiver: GossipReceiver,
}

/// Handles content and seeder discovery via iroh-gossip.
/// Each content has a gossip topic. Seeders subscribe to announce availability.
/// Downloaders subscribe to find seeders.
pub struct DiscoveryService {
    gossip: Gossip,
    /// Active topic subscriptions: content_hash -> TopicHandle
    topics: HashMap<ContentHash, TopicHandle>,
}

impl DiscoveryService {
    pub fn new(gossip: Gossip) -> Self {
        Self {
            gossip,
            topics: HashMap::new(),
        }
    }

    /// Join the gossip topic for a specific content hash.
    /// If `bootstrap` peers are known, pass them to speed up discovery.
    pub async fn join_content_topic(
        &mut self,
        hash: ContentHash,
        bootstrap: Vec<NodeId>,
    ) -> Result<()> {
        if self.topics.contains_key(&hash) {
            return Ok(()); // already joined
        }

        let topic_id = topic_for_content(&hash);
        info!(
            "Joining gossip topic for content {}",
            hex::encode(hash),
        );

        let topic = self.gossip.join(topic_id, bootstrap).await?;
        let (sender, receiver) = topic.split();

        self.topics.insert(hash, TopicHandle {
            topic_id,
            sender,
            receiver,
        });

        Ok(())
    }

    /// Announce that this node is seeding a piece of content.
    /// Must have joined the topic first via `join_content_topic`.
    pub async fn announce_seeding(
        &self,
        hash: &ContentHash,
        our_node_id: NodeId,
    ) -> Result<()> {
        let handle = self
            .topics
            .get(hash)
            .ok_or_else(|| anyhow::anyhow!("not subscribed to topic for {}", hex::encode(hash)))?;

        let msg = GossipMessage::SeederAnnounce {
            content_hash: *hash,
            node_id_bytes: *our_node_id.as_bytes(),
        };
        let encoded = serde_json::to_vec(&msg)?;
        handle.sender.broadcast(Bytes::from(encoded)).await?;

        info!("Announced seeding for {}", hex::encode(hash));
        Ok(())
    }

    /// Announce that this node is leaving the seeder swarm for content.
    pub async fn announce_leaving(
        &self,
        hash: &ContentHash,
        our_node_id: NodeId,
    ) -> Result<()> {
        let handle = self
            .topics
            .get(hash)
            .ok_or_else(|| anyhow::anyhow!("not subscribed to topic for {}", hex::encode(hash)))?;

        let msg = GossipMessage::SeederLeave {
            content_hash: *hash,
            node_id_bytes: *our_node_id.as_bytes(),
        };
        let encoded = serde_json::to_vec(&msg)?;
        handle.sender.broadcast(Bytes::from(encoded)).await?;

        Ok(())
    }

    /// Leave a content topic and stop participating in its gossip.
    pub fn leave_content_topic(&mut self, hash: &ContentHash) {
        if self.topics.remove(hash).is_some() {
            info!("Left gossip topic for {}", hex::encode(hash));
        }
    }

    /// Check if we're subscribed to a content's gossip topic.
    pub fn is_subscribed(&self, hash: &ContentHash) -> bool {
        self.topics.contains_key(hash)
    }

    /// Get the number of active topic subscriptions.
    pub fn active_topic_count(&self) -> usize {
        self.topics.len()
    }

    /// Get a reference to the underlying gossip instance.
    pub fn gossip(&self) -> &Gossip {
        &self.gossip
    }
}
