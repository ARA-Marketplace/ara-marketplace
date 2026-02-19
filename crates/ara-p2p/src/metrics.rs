use std::collections::HashMap;

use iroh::NodeId;

use crate::content::ContentHash;

/// Tracks bytes uploaded per peer per content.
/// This data is used to calculate reward weights for the distributeRewards call.
#[derive(Debug, Default)]
pub struct MetricsTracker {
    /// (content_hash, peer_node_id_bytes) => bytes_uploaded
    uploads: HashMap<(ContentHash, [u8; 32]), u64>,
}

impl MetricsTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record bytes uploaded to a peer for a specific content.
    pub fn record_upload(&mut self, content_hash: ContentHash, peer_id: NodeId, bytes: u64) {
        let key = (content_hash, *peer_id.as_bytes());
        *self.uploads.entry(key).or_insert(0) += bytes;
    }

    /// Get total bytes uploaded for a specific content across all peers.
    pub fn bytes_served_for_content(&self, content_hash: &ContentHash) -> u64 {
        self.uploads
            .iter()
            .filter(|((hash, _), _)| hash == content_hash)
            .map(|(_, &bytes)| bytes)
            .sum()
    }

    /// Get bytes uploaded to a specific peer for a specific content.
    pub fn bytes_served_to_peer(&self, content_hash: &ContentHash, peer_id: &NodeId) -> u64 {
        self.uploads
            .get(&(*content_hash, *peer_id.as_bytes()))
            .copied()
            .unwrap_or(0)
    }

    /// Get aggregated metrics for all content (for reward reporting).
    /// Returns: Vec<(content_hash, total_bytes_served)>
    pub fn all_content_metrics(&self) -> Vec<(ContentHash, u64)> {
        let mut aggregated: HashMap<ContentHash, u64> = HashMap::new();
        for ((hash, _), &bytes) in &self.uploads {
            *aggregated.entry(*hash).or_insert(0) += bytes;
        }
        aggregated.into_iter().collect()
    }

    /// Get per-peer metrics for a specific content (for reward weight calculation).
    /// Returns: Vec<(peer_node_id_bytes, bytes_served)>
    pub fn peer_metrics_for_content(&self, content_hash: &ContentHash) -> Vec<([u8; 32], u64)> {
        self.uploads
            .iter()
            .filter(|((hash, _), _)| hash == content_hash)
            .map(|((_, peer), &bytes)| (*peer, bytes))
            .collect()
    }

    /// Reset metrics for a content (after reward distribution).
    pub fn reset_content_metrics(&mut self, content_hash: &ContentHash) {
        self.uploads.retain(|(hash, _), _| hash != content_hash);
    }

    /// Reset all metrics.
    pub fn reset_all(&mut self) {
        self.uploads.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_node_id(b: u8) -> NodeId {
        let key = iroh::key::SecretKey::from_bytes(&[b; 32]);
        key.public()
    }

    #[test]
    fn test_record_and_query() {
        let mut tracker = MetricsTracker::new();
        let hash = [1u8; 32];
        let peer_a = fake_node_id(1);
        let peer_b = fake_node_id(2);

        tracker.record_upload(hash, peer_a, 1000);
        tracker.record_upload(hash, peer_b, 2000);
        tracker.record_upload(hash, peer_a, 500);

        assert_eq!(tracker.bytes_served_for_content(&hash), 3500);
        assert_eq!(tracker.bytes_served_to_peer(&hash, &peer_a), 1500);
        assert_eq!(tracker.bytes_served_to_peer(&hash, &peer_b), 2000);
    }

    #[test]
    fn test_reset() {
        let mut tracker = MetricsTracker::new();
        let hash1 = [1u8; 32];
        let hash2 = [2u8; 32];
        let peer = fake_node_id(1);

        tracker.record_upload(hash1, peer, 1000);
        tracker.record_upload(hash2, peer, 2000);

        tracker.reset_content_metrics(&hash1);
        assert_eq!(tracker.bytes_served_for_content(&hash1), 0);
        assert_eq!(tracker.bytes_served_for_content(&hash2), 2000);
    }

    #[test]
    fn test_peer_metrics() {
        let mut tracker = MetricsTracker::new();
        let hash = [1u8; 32];
        let peer_a = fake_node_id(1);
        let peer_b = fake_node_id(2);

        tracker.record_upload(hash, peer_a, 1000);
        tracker.record_upload(hash, peer_b, 3000);

        let metrics = tracker.peer_metrics_for_content(&hash);
        assert_eq!(metrics.len(), 2);

        let total: u64 = metrics.iter().map(|(_, b)| b).sum();
        assert_eq!(total, 4000);
    }
}
