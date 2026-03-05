//! # Ara SDK
//!
//! Programmatic access to the Ara Marketplace — publish, purchase, seed, stake,
//! and manage content without the Tauri desktop app.
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
//!     .signer(PrivateKeySigner::new("0x..."))
//!     .build()
//!     .await?;
//!
//! // Prepare stake transactions (returns calldata for signing)
//! let txs = client.staking().prepare_stake("100")?;
//! # Ok(())
//! # }
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
