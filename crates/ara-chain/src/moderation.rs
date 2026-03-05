use alloy::primitives::{Address, FixedBytes};
use alloy::providers::Provider;
use alloy::sol_types::SolCall;
use anyhow::Result;

use crate::contracts::IAraModeration;

/// Wrapper for AraModeration contract interactions.
pub struct ModerationClient<P> {
    address: Address,
    provider: P,
}

impl<P: Provider + Clone> ModerationClient<P> {
    pub fn new(address: Address, provider: P) -> Self {
        Self { address, provider }
    }

    pub fn address(&self) -> Address {
        self.address
    }

    /// Check if content is tagged NSFW.
    pub async fn is_nsfw(&self, content_id: FixedBytes<32>) -> Result<bool> {
        let contract = IAraModeration::new(self.address, &self.provider);
        let result = contract.isNsfw(content_id).call().await?;
        Ok(result)
    }

    /// Check if content is permanently purged.
    pub async fn is_purged(&self, content_id: FixedBytes<32>) -> Result<bool> {
        let contract = IAraModeration::new(self.address, &self.provider);
        let result = contract.isPurged(content_id).call().await?;
        Ok(result)
    }

    /// Get proposal status (0=None, 1=Active, 2=Upheld, 3=Dismissed, 4=Purged).
    pub async fn get_proposal_status(&self, content_id: FixedBytes<32>) -> Result<u8> {
        let contract = IAraModeration::new(self.address, &self.provider);
        let result = contract.getProposalStatus(content_id).call().await?;
        Ok(result)
    }

    /// Check if user has already flagged this content.
    pub async fn has_flagged(&self, content_id: FixedBytes<32>, user: Address) -> Result<bool> {
        let contract = IAraModeration::new(self.address, &self.provider);
        let result = contract.hasFlagged(content_id, user).call().await?;
        Ok(result)
    }

    /// Check if user has already voted on this proposal.
    pub async fn has_voted(&self, content_id: FixedBytes<32>, user: Address) -> Result<bool> {
        let contract = IAraModeration::new(self.address, &self.provider);
        let result = contract.hasVoted(content_id, user).call().await?;
        Ok(result)
    }
}

// ─── Calldata builders (no provider needed) ─────────────────────────

impl<P> ModerationClient<P> {
    /// Build calldata for flagContent(contentId, reason, isEmergency)
    pub fn flag_content_calldata(content_id: FixedBytes<32>, reason: u8, is_emergency: bool) -> Vec<u8> {
        IAraModeration::flagContentCall {
            contentId: content_id,
            reason,
            isEmergency: is_emergency,
        }
        .abi_encode()
    }

    /// Build calldata for vote(contentId, uphold)
    pub fn vote_calldata(content_id: FixedBytes<32>, uphold: bool) -> Vec<u8> {
        IAraModeration::voteCall {
            contentId: content_id,
            uphold,
        }
        .abi_encode()
    }

    /// Build calldata for resolveFlag(contentId)
    pub fn resolve_flag_calldata(content_id: FixedBytes<32>) -> Vec<u8> {
        IAraModeration::resolveFlagCall {
            contentId: content_id,
        }
        .abi_encode()
    }

    /// Build calldata for appeal(contentId)
    pub fn appeal_calldata(content_id: FixedBytes<32>) -> Vec<u8> {
        IAraModeration::appealCall {
            contentId: content_id,
        }
        .abi_encode()
    }

    /// Build calldata for setNsfw(contentId, isNsfw)
    pub fn set_nsfw_calldata(content_id: FixedBytes<32>, is_nsfw: bool) -> Vec<u8> {
        IAraModeration::setNsfwCall {
            contentId: content_id,
            isNsfw: is_nsfw,
        }
        .abi_encode()
    }

    /// Build calldata for voteNsfw(contentId)
    pub fn vote_nsfw_calldata(content_id: FixedBytes<32>) -> Vec<u8> {
        IAraModeration::voteNsfwCall {
            contentId: content_id,
        }
        .abi_encode()
    }
}
