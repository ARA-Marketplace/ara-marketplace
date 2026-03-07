//! # Ara SDK
//!
//! Programmatic access to the Ara Marketplace — publish, purchase, stake,
//! and manage content without the Tauri desktop app.
//!
//! ## Overview
//!
//! The SDK follows a **prepare → execute → confirm** pattern:
//!
//! 1. **Prepare** — Build unsigned transaction calldata (`prepare_*` methods)
//! 2. **Execute** — Sign and broadcast via [`AraClient::execute_transactions`]
//! 3. **Confirm** — Update local DB state (`confirm_*` methods)
//!
//! All on-chain operations return [`TransactionRequest`] structs containing the
//! contract address, ABI-encoded calldata, ETH value, and a human-readable
//! description. You can use the built-in [`PrivateKeySigner`] or implement the
//! [`Signer`] trait for hardware wallets, remote signers, or browser wallets.
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use ara_sdk::{AraClient, PrivateKeySigner};
//! use ara_core::config::AppConfig;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let client = AraClient::builder()
//!     .config(AppConfig::default())
//!     .signer(PrivateKeySigner::new("0xYOUR_KEY", "https://eth-sepolia.g.alchemy.com/v2/KEY"))
//!     .build_in_memory()
//!     .await?;
//!
//! // Check balances
//! let addr = client.wallet_address().unwrap();
//! let balances = client.get_balances(addr).await?;
//! println!("ETH: {}, ARA: {}, Staked: {}",
//!     balances.eth_display, balances.ara_display, balances.staked_display);
//! # Ok(())
//! # }
//! ```
//!
//! ## Operations
//!
//! Access domain-specific operations through accessor methods on [`AraClient`]:
//!
//! | Accessor | Module | Operations |
//! |----------|--------|------------|
//! | [`client.content()`](AraClient::content) | [`content`] | Publish, update, delist, search, query |
//! | [`client.marketplace()`](AraClient::marketplace) | [`marketplace`] | Purchase, resale, delivery rewards |
//! | [`client.staking()`](AraClient::staking) | [`staking`] | Stake/unstake ARA, claim rewards |
//! | [`client.collections()`](AraClient::collections) | [`collections`] | Create/manage on-chain collections |
//! | [`client.names()`](AraClient::names) | [`names`] | Register/remove display names |
//! | [`client.moderation()`](AraClient::moderation) | [`moderation`] | Flag, vote, NSFW tagging |
//! | [`client.analytics()`](AraClient::analytics) | [`analytics`] | Price history, trending, stats |
//! | [`client.sync()`](AraClient::sync) | [`sync`] | Sync chain events to local DB |
//!
//! ## Staking Example
//!
//! ```rust,no_run
//! # use ara_sdk::AraClient;
//! # async fn example(client: &AraClient) -> anyhow::Result<()> {
//! // Stake 100 ARA (produces approve + stake transactions)
//! let txs = client.staking().prepare_stake("100")?;
//! let hashes = client.execute_transactions(&txs).await?;
//! println!("Staked! tx: {}", hashes[1]);
//!
//! // Check stake info
//! let addr = client.wallet_address().unwrap();
//! let info = client.staking().get_stake_info(addr).await?;
//! println!("Staked: {} wei", info.general_balance_wei);
//! # Ok(())
//! # }
//! ```
//!
//! ## Publishing Content
//!
//! ```rust,no_run
//! # use ara_sdk::AraClient;
//! # use alloy::primitives::FixedBytes;
//! # async fn example(client: &AraClient) -> anyhow::Result<()> {
//! let content_hash = FixedBytes::<32>::ZERO; // BLAKE3 hash of your file
//! let metadata = serde_json::json!({
//!     "title": "My Content",
//!     "description": "A test publication",
//!     "content_type": "application/pdf",
//!     "filename": "document.pdf",
//!     "file_size": 1048576
//! });
//!
//! let result = client.content().prepare_publish(
//!     content_hash,
//!     metadata.to_string(),
//!     "0.01",       // price in ETH
//!     1048576,      // file size bytes
//!     100,          // max supply (0 = unlimited)
//!     500,          // royalty bps (5%)
//!     None,         // payment token (None = ETH)
//! ).await?;
//!
//! let hashes = client.execute_transactions(&result.transactions).await?;
//! println!("Published! tx: {}", hashes[0]);
//! # Ok(())
//! # }
//! ```
//!
//! ## Purchasing Content
//!
//! ```rust,no_run
//! # use ara_sdk::AraClient;
//! # async fn example(client: &AraClient) -> anyhow::Result<()> {
//! // Sync content from chain first
//! client.sync().sync_content().await?;
//!
//! // Search for content
//! let items = client.content().search("music", 10).await?;
//! if let Some(item) = items.first() {
//!     // Prepare purchase (looks up price from local DB)
//!     let prep = client.marketplace().prepare_purchase(&item.content_id).await?;
//!     let hashes = client.execute_transactions(&prep.transactions).await?;
//!
//!     // Confirm in local DB
//!     let addr = client.wallet_address().unwrap();
//!     client.marketplace().confirm_purchase(
//!         &item.content_id,
//!         &format!("{addr:#x}"),
//!         &item.price_wei,
//!         hashes.last().unwrap(),
//!     ).await?;
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Read-Only Mode
//!
//! Without a signer, the SDK can still query on-chain data and local DB:
//!
//! ```rust,no_run
//! # use ara_sdk::AraClient;
//! # async fn example() -> anyhow::Result<()> {
//! let client = AraClient::builder()
//!     .build_in_memory()
//!     .await?;
//!
//! // Sync and search (no signer needed)
//! client.sync().sync_content().await?;
//! let items = client.content().search("", 50).await?;
//!
//! // Prepare transactions returns calldata you can sign externally
//! let txs = client.staking().prepare_stake("10")?;
//! println!("To: {}, Data: {}", txs[0].to, txs[0].data);
//! # Ok(())
//! # }
//! ```
//!
//! ## Custom Signers
//!
//! Implement the [`Signer`] trait for custom wallet integrations:
//!
//! ```rust,no_run
//! use ara_sdk::{Signer, TransactionRequest};
//! use alloy::primitives::Address;
//!
//! struct MyHardwareSigner { /* ... */ }
//!
//! #[async_trait::async_trait]
//! impl Signer for MyHardwareSigner {
//!     async fn sign_and_send(&self, tx: &TransactionRequest) -> anyhow::Result<String> {
//!         // Send to hardware wallet, get signature, broadcast
//!         todo!()
//!     }
//!     async fn sign_typed_data(&self, _d: &str, _t: &str, _v: &str) -> anyhow::Result<Vec<u8>> {
//!         todo!()
//!     }
//!     fn address(&self) -> Address { todo!() }
//! }
//! ```

pub mod client;
pub mod content;
pub mod marketplace;
pub mod staking;
pub mod collections;
pub mod names;
pub mod moderation;
pub mod analytics;
pub mod sync;
pub mod signer;
pub mod types;

pub use client::{AraClient, AraClientBuilder};
pub use signer::{Signer, PrivateKeySigner};
pub use types::TransactionRequest;
