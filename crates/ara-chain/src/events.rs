use alloy::primitives::{Address, FixedBytes, U256};
use alloy::providers::Provider;
use alloy::rpc::types::Filter;
use alloy::sol_types::SolEvent;
use anyhow::Result;
use tracing::info;

use crate::contracts::{IAraStaking, IContentRegistry, IMarketplace};

/// Decoded event from the Ara Marketplace contracts.
#[derive(Debug, Clone)]
pub enum AraEvent {
    ContentPublished {
        content_id: FixedBytes<32>,
        creator: Address,
        content_hash: FixedBytes<32>,
        metadata_uri: String,
        price_wei: U256,
        file_size: U256,
    },
    ContentUpdated {
        content_id: FixedBytes<32>,
        new_price_wei: U256,
        new_metadata_uri: String,
    },
    ContentDelisted {
        content_id: FixedBytes<32>,
    },
    ContentPurchased {
        content_id: FixedBytes<32>,
        buyer: Address,
        price_paid: U256,
        creator_payment: U256,
        reward_amount: U256,
    },
    DeliveryRewardClaimed {
        content_id: FixedBytes<32>,
        seeder: Address,
        buyer: Address,
        amount: U256,
        bytes_served: U256,
    },
    RewardsClaimed {
        seeder: Address,
        total_amount: U256,
        receipt_count: U256,
    },
    Staked {
        user: Address,
        amount: U256,
    },
    Unstaked {
        user: Address,
        amount: U256,
    },
    ContentStakeAdded {
        user: Address,
        content_id: FixedBytes<32>,
        amount: U256,
    },
    ContentStakeRemoved {
        user: Address,
        content_id: FixedBytes<32>,
        amount: U256,
    },
}

/// Fetched event with block metadata.
#[derive(Debug, Clone)]
pub struct IndexedEvent {
    pub block_number: u64,
    pub tx_hash: Option<FixedBytes<32>>,
    pub log_index: Option<u64>,
    pub event: AraEvent,
}

/// Fetches and decodes on-chain events from the Ara contracts.
/// Returns typed events for the caller to store/process.
pub struct EventIndexer<P> {
    registry_address: Address,
    marketplace_address: Address,
    staking_address: Address,
    provider: P,
}

impl<P: Provider + Clone> EventIndexer<P> {
    pub fn new(
        registry_address: Address,
        marketplace_address: Address,
        staking_address: Address,
        provider: P,
    ) -> Self {
        Self {
            registry_address,
            marketplace_address,
            staking_address,
            provider,
        }
    }

    /// Fetch all Ara contract events in a block range.
    /// Returns events sorted by block number.
    pub async fn fetch_events(
        &self,
        from_block: u64,
        to_block: Option<u64>,
    ) -> Result<Vec<IndexedEvent>> {
        info!("Fetching events from block {}", from_block);

        let mut filter = Filter::new()
            .address(vec![
                self.registry_address,
                self.marketplace_address,
                self.staking_address,
            ])
            .from_block(from_block);

        if let Some(to) = to_block {
            filter = filter.to_block(to);
        }

        let logs = self.provider.get_logs(&filter).await?;
        info!("Fetched {} raw logs", logs.len());

        let mut events = Vec::new();

        for log in logs {
            let block_number = log.block_number.unwrap_or(0);
            let tx_hash = log.transaction_hash;
            let log_index = log.log_index;

            if let Some(event) = self.decode_log(&log.inner) {
                events.push(IndexedEvent {
                    block_number,
                    tx_hash,
                    log_index,
                    event,
                });
            }
        }

        // Sort by block number, then log index
        events.sort_by_key(|e| (e.block_number, e.log_index.unwrap_or(0)));

        info!("Decoded {} events", events.len());
        Ok(events)
    }

    /// Fetch only content-related events (published, updated, delisted).
    pub async fn fetch_content_events(
        &self,
        from_block: u64,
        to_block: Option<u64>,
    ) -> Result<Vec<IndexedEvent>> {
        let mut filter = Filter::new()
            .address(self.registry_address)
            .from_block(from_block);

        if let Some(to) = to_block {
            filter = filter.to_block(to);
        }

        let logs = self.provider.get_logs(&filter).await?;
        let mut events = Vec::new();

        for log in logs {
            let block_number = log.block_number.unwrap_or(0);
            let tx_hash = log.transaction_hash;
            let log_index = log.log_index;

            if let Some(event) = self.decode_log(&log.inner) {
                events.push(IndexedEvent {
                    block_number,
                    tx_hash,
                    log_index,
                    event,
                });
            }
        }

        events.sort_by_key(|e| (e.block_number, e.log_index.unwrap_or(0)));
        Ok(events)
    }

    /// Fetch only purchase and reward events.
    pub async fn fetch_marketplace_events(
        &self,
        from_block: u64,
        to_block: Option<u64>,
    ) -> Result<Vec<IndexedEvent>> {
        let mut filter = Filter::new()
            .address(self.marketplace_address)
            .from_block(from_block);

        if let Some(to) = to_block {
            filter = filter.to_block(to);
        }

        let logs = self.provider.get_logs(&filter).await?;
        let mut events = Vec::new();

        for log in logs {
            let block_number = log.block_number.unwrap_or(0);
            let tx_hash = log.transaction_hash;
            let log_index = log.log_index;

            if let Some(event) = self.decode_log(&log.inner) {
                events.push(IndexedEvent {
                    block_number,
                    tx_hash,
                    log_index,
                    event,
                });
            }
        }

        events.sort_by_key(|e| (e.block_number, e.log_index.unwrap_or(0)));
        Ok(events)
    }

    /// Try to decode a raw log into a typed AraEvent.
    fn decode_log(&self, log: &alloy::primitives::Log) -> Option<AraEvent> {
        // Try ContentRegistry events
        if let Ok(e) = IContentRegistry::ContentPublished::decode_log(log) {
            return Some(AraEvent::ContentPublished {
                content_id: e.contentId,
                creator: e.creator,
                content_hash: e.contentHash,
                metadata_uri: e.metadataURI.clone(),
                price_wei: e.priceWei,
                file_size: e.fileSize,
            });
        }
        if let Ok(e) = IContentRegistry::ContentUpdated::decode_log(log) {
            return Some(AraEvent::ContentUpdated {
                content_id: e.contentId,
                new_price_wei: e.newPriceWei,
                new_metadata_uri: e.newMetadataURI.clone(),
            });
        }
        if let Ok(e) = IContentRegistry::ContentDelisted::decode_log(log) {
            return Some(AraEvent::ContentDelisted {
                content_id: e.contentId,
            });
        }

        // Try Marketplace events
        if let Ok(e) = IMarketplace::ContentPurchased::decode_log(log) {
            return Some(AraEvent::ContentPurchased {
                content_id: e.contentId,
                buyer: e.buyer,
                price_paid: e.pricePaid,
                creator_payment: e.creatorPayment,
                reward_amount: e.rewardAmount,
            });
        }
        if let Ok(e) = IMarketplace::DeliveryRewardClaimed::decode_log(log) {
            return Some(AraEvent::DeliveryRewardClaimed {
                content_id: e.contentId,
                seeder: e.seeder,
                buyer: e.buyer,
                amount: e.amount,
                bytes_served: e.bytesServed,
            });
        }
        if let Ok(e) = IMarketplace::RewardsClaimed::decode_log(log) {
            return Some(AraEvent::RewardsClaimed {
                seeder: e.seeder,
                total_amount: e.totalAmount,
                receipt_count: e.receiptCount,
            });
        }

        // Try Staking events
        if let Ok(e) = IAraStaking::Staked::decode_log(log) {
            return Some(AraEvent::Staked {
                user: e.user,
                amount: e.amount,
            });
        }
        if let Ok(e) = IAraStaking::Unstaked::decode_log(log) {
            return Some(AraEvent::Unstaked {
                user: e.user,
                amount: e.amount,
            });
        }
        if let Ok(e) = IAraStaking::ContentStakeAdded::decode_log(log) {
            return Some(AraEvent::ContentStakeAdded {
                user: e.user,
                content_id: e.contentId,
                amount: e.amount,
            });
        }
        if let Ok(e) = IAraStaking::ContentStakeRemoved::decode_log(log) {
            return Some(AraEvent::ContentStakeRemoved {
                user: e.user,
                content_id: e.contentId,
                amount: e.amount,
            });
        }

        None
    }
}
