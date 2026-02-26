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
    /// Buyer-signed proof that they received content from a specific seeder.
    /// The buyer signs EIP-712 DeliveryReceipt(contentId, seederEthAddress, bytesServed, timestamp)
    /// with their Ethereum wallet. Seeders submit these on-chain to claim proportional rewards.
    DeliveryReceipt {
        /// On-chain keccak256 content ID (bytes32)
        content_id: [u8; 32],
        /// Seeder's Ethereum address (20 bytes)
        seeder_eth_address: [u8; 20],
        /// Buyer's Ethereum address (20 bytes) — for routing/deduplication
        buyer_eth_address: [u8; 20],
        /// 65-byte EIP-712 ECDSA signature (r || s || v) — Vec to satisfy serde bounds
        signature: Vec<u8>,
        /// Unix timestamp when the receipt was signed
        timestamp: u64,
        /// Number of bytes served by this seeder (for proportional reward calculation)
        bytes_served: u64,
    },
    /// Links a seeder's iroh NodeId to their Ethereum address.
    /// The seeder signs their eth_address with their iroh Ed25519 key so that
    /// creators can map NodeId → ETH address for reward distribution.
    SeederIdentity {
        /// Seeder's iroh NodeId (32 bytes, Ed25519 public key)
        node_id: [u8; 32],
        /// Seeder's Ethereum address (20 bytes)
        eth_address: [u8; 20],
        /// Ed25519 signature — Vec to satisfy serde bounds
        signature: Vec<u8>,
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

    /// Broadcast a delivery receipt on the gossip topic for the given content hash.
    /// Call this after a buyer signs their receipt so that the seeder and creator collect it.
    pub async fn broadcast_delivery_receipt(
        &self,
        content_hash: &ContentHash,
        content_id: [u8; 32],
        seeder_eth_address: [u8; 20],
        buyer_eth_address: [u8; 20],
        signature: Vec<u8>,
        timestamp: u64,
        bytes_served: u64,
    ) -> Result<()> {
        let handle = self
            .topics
            .get(content_hash)
            .ok_or_else(|| anyhow::anyhow!("not subscribed to topic for {}", hex::encode(content_hash)))?;

        let msg = GossipMessage::DeliveryReceipt {
            content_id,
            seeder_eth_address,
            buyer_eth_address,
            signature,
            timestamp,
            bytes_served,
        };
        let encoded = serde_json::to_vec(&msg)?;
        handle.sender.broadcast(Bytes::from(encoded)).await?;
        Ok(())
    }

    /// Broadcast a SeederIdentity message on all active content topics.
    /// Seeders call this on startup to let creators map NodeId → ETH address for reward calculation.
    pub async fn broadcast_seeder_identity(
        &self,
        node_id: [u8; 32],
        eth_address: [u8; 20],
        signature: Vec<u8>,
    ) -> Result<()> {
        let msg = GossipMessage::SeederIdentity { node_id, eth_address, signature };
        let encoded = serde_json::to_vec(&msg)?;
        let bytes = Bytes::from(encoded);

        for handle in self.topics.values() {
            // Best-effort: ignore errors on individual topics
            let _ = handle.sender.broadcast(bytes.clone()).await;
        }
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
