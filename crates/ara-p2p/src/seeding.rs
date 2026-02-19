use std::collections::HashSet;

use anyhow::Result;
use iroh::NodeId;
use tracing::info;

use crate::content::ContentHash;
use crate::discovery::DiscoveryService;

/// Manages which content this node is actively seeding (serving to the network).
///
/// Seeding means:
/// 1. The blob exists in the local iroh store (auto-served to any peer that connects)
/// 2. We've joined the gossip topic for discovery
/// 3. We've announced our availability as a seeder
pub struct SeedingManager {
    /// Content hashes we're actively seeding.
    active_seeds: HashSet<ContentHash>,
    /// Our node's public identity.
    node_id: NodeId,
}

impl SeedingManager {
    pub fn new(node_id: NodeId) -> Self {
        Self {
            active_seeds: HashSet::new(),
            node_id,
        }
    }

    /// Start seeding a piece of content.
    /// Joins the gossip topic and announces availability.
    /// The blob must already exist in the local store (imported or downloaded).
    pub async fn start_seeding(
        &mut self,
        hash: ContentHash,
        discovery: &mut DiscoveryService,
    ) -> Result<()> {
        if self.active_seeds.contains(&hash) {
            return Ok(());
        }

        info!("Starting to seed: {}", hex::encode(hash));

        // Join the gossip topic for this content
        discovery.join_content_topic(hash, vec![]).await?;

        // Announce that we're seeding
        discovery.announce_seeding(&hash, self.node_id).await?;

        self.active_seeds.insert(hash);
        info!(
            "Now seeding {} content items",
            self.active_seeds.len(),
        );
        Ok(())
    }

    /// Stop seeding a piece of content.
    /// Announces departure and leaves the gossip topic.
    pub async fn stop_seeding(
        &mut self,
        hash: &ContentHash,
        discovery: &mut DiscoveryService,
    ) -> Result<()> {
        if !self.active_seeds.contains(hash) {
            return Ok(());
        }

        info!("Stopping seed: {}", hex::encode(hash));

        // Announce that we're leaving (best-effort, ignore errors)
        let _ = discovery.announce_leaving(hash, self.node_id).await;

        // Leave the gossip topic
        discovery.leave_content_topic(hash);

        self.active_seeds.remove(hash);
        Ok(())
    }

    /// Check if we're currently seeding a specific content.
    pub fn is_seeding(&self, hash: &ContentHash) -> bool {
        self.active_seeds.contains(hash)
    }

    /// Get all content hashes we're currently seeding.
    pub fn active_seeds(&self) -> Vec<ContentHash> {
        self.active_seeds.iter().copied().collect()
    }

    /// Get the number of content items being seeded.
    pub fn seed_count(&self) -> usize {
        self.active_seeds.len()
    }
}
