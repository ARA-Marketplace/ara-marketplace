use alloy::primitives::{Address, FixedBytes};
use anyhow::Result;

use ara_chain::moderation::ModerationClient;

use crate::client::AraClient;
use crate::types::{hex_encode, TransactionRequest};

/// Moderation operations: flag, vote, resolve, appeal, NSFW tagging.
pub struct ModerationOps<'a> {
    pub(crate) client: &'a AraClient,
}

impl ModerationOps<'_> {
    /// Prepare a flag-content transaction.
    /// `reason`: 0=copyright, 1=spam, 2=malware, 3=fraud, 4=other_illegal
    pub fn prepare_flag(
        &self,
        content_id: FixedBytes<32>,
        reason: u8,
        is_emergency: bool,
    ) -> Result<Vec<TransactionRequest>> {
        let eth = &self.client.config.ethereum;
        let moderation_addr: Address = eth.moderation_address.parse()?;

        let calldata =
            ModerationClient::<()>::flag_content_calldata(content_id, reason, is_emergency);

        Ok(vec![TransactionRequest {
            to: format!("{moderation_addr:#x}"),
            data: hex_encode(&calldata),
            value: "0x0".to_string(),
            description: format!(
                "Flag content{}",
                if is_emergency { " (emergency)" } else { "" }
            ),
        }])
    }

    /// Prepare a vote on a moderation proposal.
    pub fn prepare_vote(
        &self,
        content_id: FixedBytes<32>,
        uphold: bool,
    ) -> Result<Vec<TransactionRequest>> {
        let eth = &self.client.config.ethereum;
        let moderation_addr: Address = eth.moderation_address.parse()?;

        let calldata = ModerationClient::<()>::vote_calldata(content_id, uphold);

        Ok(vec![TransactionRequest {
            to: format!("{moderation_addr:#x}"),
            data: hex_encode(&calldata),
            value: "0x0".to_string(),
            description: format!(
                "Vote to {} flag",
                if uphold { "uphold" } else { "dismiss" }
            ),
        }])
    }

    /// Prepare a resolve-flag transaction (after voting period).
    pub fn prepare_resolve(
        &self,
        content_id: FixedBytes<32>,
    ) -> Result<Vec<TransactionRequest>> {
        let eth = &self.client.config.ethereum;
        let moderation_addr: Address = eth.moderation_address.parse()?;

        let calldata = ModerationClient::<()>::resolve_flag_calldata(content_id);

        Ok(vec![TransactionRequest {
            to: format!("{moderation_addr:#x}"),
            data: hex_encode(&calldata),
            value: "0x0".to_string(),
            description: "Resolve moderation flag".to_string(),
        }])
    }

    /// Prepare an appeal transaction (creator-only).
    pub fn prepare_appeal(
        &self,
        content_id: FixedBytes<32>,
    ) -> Result<Vec<TransactionRequest>> {
        let eth = &self.client.config.ethereum;
        let moderation_addr: Address = eth.moderation_address.parse()?;

        let calldata = ModerationClient::<()>::appeal_calldata(content_id);

        Ok(vec![TransactionRequest {
            to: format!("{moderation_addr:#x}"),
            data: hex_encode(&calldata),
            value: "0x0".to_string(),
            description: "Appeal moderation flag".to_string(),
        }])
    }

    /// Prepare a set-NSFW transaction (creator self-tagging).
    pub fn prepare_set_nsfw(
        &self,
        content_id: FixedBytes<32>,
        is_nsfw: bool,
    ) -> Result<Vec<TransactionRequest>> {
        let eth = &self.client.config.ethereum;
        let moderation_addr: Address = eth.moderation_address.parse()?;

        let calldata = ModerationClient::<()>::set_nsfw_calldata(content_id, is_nsfw);

        Ok(vec![TransactionRequest {
            to: format!("{moderation_addr:#x}"),
            data: hex_encode(&calldata),
            value: "0x0".to_string(),
            description: format!(
                "{} NSFW tag",
                if is_nsfw { "Set" } else { "Remove" }
            ),
        }])
    }

    /// Prepare a vote-NSFW transaction (community NSFW tagging).
    pub fn prepare_vote_nsfw(
        &self,
        content_id: FixedBytes<32>,
    ) -> Result<Vec<TransactionRequest>> {
        let eth = &self.client.config.ethereum;
        let moderation_addr: Address = eth.moderation_address.parse()?;

        let calldata = ModerationClient::<()>::vote_nsfw_calldata(content_id);

        Ok(vec![TransactionRequest {
            to: format!("{moderation_addr:#x}"),
            data: hex_encode(&calldata),
            value: "0x0".to_string(),
            description: "Vote content as NSFW".to_string(),
        }])
    }

    /// Check if content is tagged NSFW on-chain.
    pub async fn is_nsfw(&self, content_id: FixedBytes<32>) -> Result<bool> {
        let chain = self.client.chain_client()?;
        chain.moderation.is_nsfw(content_id).await
    }

    /// Check if content is permanently purged on-chain.
    pub async fn is_purged(&self, content_id: FixedBytes<32>) -> Result<bool> {
        let chain = self.client.chain_client()?;
        chain.moderation.is_purged(content_id).await
    }

    /// Get proposal status (0=None, 1=Active, 2=Upheld, 3=Dismissed, 4=Purged).
    pub async fn get_proposal_status(&self, content_id: FixedBytes<32>) -> Result<u8> {
        let chain = self.client.chain_client()?;
        chain.moderation.get_proposal_status(content_id).await
    }

    /// Check if a user has already flagged this content.
    pub async fn has_flagged(
        &self,
        content_id: FixedBytes<32>,
        user: Address,
    ) -> Result<bool> {
        let chain = self.client.chain_client()?;
        chain.moderation.has_flagged(content_id, user).await
    }

    /// Check if a user has already voted on this proposal.
    pub async fn has_voted(
        &self,
        content_id: FixedBytes<32>,
        user: Address,
    ) -> Result<bool> {
        let chain = self.client.chain_client()?;
        chain.moderation.has_voted(content_id, user).await
    }
}
