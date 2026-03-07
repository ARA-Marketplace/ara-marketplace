# Ara SDK

Programmatic Rust SDK for the [Ara Marketplace](https://github.com/AraBlocks/ara-marketplace) — publish, purchase, stake, and manage content on Ethereum without the desktop app.

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
ara-sdk = { path = "crates/ara-sdk" }  # or git URL when published
ara-core = { path = "crates/ara-core" }
tokio = { version = "1", features = ["full"] }
alloy = { version = "1", features = ["full"] }
```

## Quick Start

```rust
use ara_sdk::{AraClient, PrivateKeySigner};
use ara_core::config::AppConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Build client with signer
    let client = AraClient::builder()
        .config(AppConfig::default())
        .signer(PrivateKeySigner::new(
            "0xYOUR_PRIVATE_KEY",
            "https://eth-sepolia.g.alchemy.com/v2/YOUR_KEY",
        ))
        .build_in_memory()
        .await?;

    // Check balances
    let addr = client.wallet_address().unwrap();
    let bal = client.get_balances(addr).await?;
    println!("ETH: {}, ARA: {}", bal.eth_display, bal.ara_display);

    Ok(())
}
```

## Architecture

The SDK follows a **prepare -> execute -> confirm** pattern:

1. **Prepare** — `prepare_*()` methods build unsigned `TransactionRequest` structs containing contract address, ABI-encoded calldata, ETH value, and description
2. **Execute** — `client.execute_transactions()` signs and broadcasts via the configured signer
3. **Confirm** — `confirm_*()` methods update the local SQLite database with the result

This separation lets you:
- Use `prepare_*` without a signer for calldata generation (pass to MetaMask, hardware wallets, etc.)
- Use the built-in `PrivateKeySigner` for automated/scripted workflows
- Implement the `Signer` trait for custom wallet integrations

### Client Structure

```
AraClient
  .content()       -> ContentOps       # Publish, update, delist, search
  .marketplace()   -> MarketplaceOps   # Purchase, resale, delivery rewards
  .staking()       -> StakingOps       # Stake/unstake ARA, claim rewards
  .collections()   -> CollectionOps    # On-chain content collections
  .names()         -> NameOps          # Display name registry
  .moderation()    -> ModerationOps    # Flag, vote, NSFW tagging
  .analytics()     -> AnalyticsOps     # Price history, trending, stats
  .sync()          -> SyncOps          # Sync chain events to local DB
```

## Usage Examples

### Staking ARA

```rust
// Stake 100 ARA (generates approve + stake transactions)
let txs = client.staking().prepare_stake("100")?;
let hashes = client.execute_transactions(&txs).await?;
println!("Staked! Approve: {}, Stake: {}", hashes[0], hashes[1]);

// Check stake info
let info = client.staking().get_stake_info(addr).await?;
println!("Staked: {} wei, Earned: {} wei",
    info.general_balance_wei, info.eth_reward_earned_wei);

// Unstake
let txs = client.staking().prepare_unstake("50")?;
client.execute_transactions(&txs).await?;
```

### Publishing Content

```rust
use alloy::primitives::FixedBytes;

// BLAKE3 hash of your file (use blake3 crate)
let content_hash: FixedBytes<32> = /* your hash */;

let metadata = serde_json::json!({
    "title": "My Song",
    "description": "An original composition",
    "content_type": "audio/mpeg",
    "filename": "song.mp3",
    "file_size": 5242880,
});

let result = client.content().prepare_publish(
    content_hash,
    metadata.to_string(),
    "0.01",       // price in ETH
    5242880,      // file size in bytes
    100,          // max supply (0 = unlimited)
    500,          // royalty in basis points (5%)
    None,         // payment token (None = ETH)
).await?;

let hashes = client.execute_transactions(&result.transactions).await?;

// After getting contentId from the tx receipt's ContentPublished event:
client.content().confirm_publish(
    "0xCONTENT_ID",
    &format!("{content_hash:#x}"),
    &format!("{addr:#x}"),
    &metadata.to_string(),
    "10000000000000000",  // price in wei
).await?;
```

### Purchasing Content

```rust
// Sync content from chain to local DB
client.sync().sync_content().await?;

// Search for content
let items = client.content().search("music", 10).await?;

for item in &items {
    println!("{}: {} {} (by {})",
        item.title, item.price_display, item.price_unit, item.creator);
}

// Purchase
if let Some(item) = items.first() {
    let prep = client.marketplace().prepare_purchase(&item.content_id).await?;
    let hashes = client.execute_transactions(&prep.transactions).await?;

    client.marketplace().confirm_purchase(
        &item.content_id,
        &format!("{addr:#x}"),
        &item.price_wei,
        hashes.last().unwrap(),
    ).await?;
}
```

### Collections

```rust
// Create a collection
let txs = client.collections().prepare_create(
    "My Playlist", "Best tracks of 2026", ""
).unwrap();
client.execute_transactions(&txs).await?;

// List your collections
let ids = client.collections().get_creator_collections(addr).await?;
for id in &ids {
    let (owner, name, desc, banner, item_count, active) =
        client.collections().get_collection(*id).await?;
    println!("{}: {} ({} items)", id, name, item_count);
}
```

### Display Names

```rust
// Check availability
let available = client.names().check_available("alice").await?;

// Register
let txs = client.names().prepare_register("alice")?;
client.execute_transactions(&txs).await?;
client.names().confirm_register(&format!("{addr:#x}"), "alice").await?;

// Lookup
let name = client.names().get_name(addr).await?;
println!("Display name: {}", name);

// Remove
let txs = client.names().prepare_remove()?;
client.execute_transactions(&txs).await?;
```

### Syncing and Analytics

```rust
// Sync content events
let result = client.sync().sync_content().await?;
println!("{} new, {} updated, {} delisted", result.new_content, result.updated, result.delisted);

// Sync marketplace events (purchases, listings)
let rewards = client.sync().sync_rewards().await?;
println!("{} purchases, {} listings", rewards.purchases_found, rewards.listings_found);

// Analytics
let overview = client.analytics().get_overview().await?;
println!("{} total sales, {} ETH volume", overview.total_sales, overview.total_volume_eth);

let trending = client.analytics().get_trending(10).await?;
let collectors = client.analytics().get_top_collectors(10).await?;
```

### Read-Only Mode (No Signer)

```rust
let client = AraClient::builder()
    .build_in_memory()
    .await?;

// Query chain data
client.sync().sync_content().await?;
let items = client.content().search("", 50).await?;

// Prepare calldata for external signing
let txs = client.staking().prepare_stake("10")?;
println!("Send to: {}", txs[0].to);
println!("Calldata: {}", txs[0].data);
println!("Value: {}", txs[0].value);
```

### Custom Signer

```rust
use ara_sdk::{Signer, TransactionRequest};
use alloy::primitives::Address;

struct LedgerSigner { /* ... */ }

#[async_trait::async_trait]
impl Signer for LedgerSigner {
    async fn sign_and_send(&self, tx: &TransactionRequest) -> anyhow::Result<String> {
        // Parse tx.to, tx.data, tx.value
        // Send to Ledger for signing
        // Broadcast signed transaction
        // Return tx hash
        todo!()
    }

    async fn sign_typed_data(&self, domain: &str, types: &str, value: &str)
        -> anyhow::Result<Vec<u8>>
    {
        todo!()
    }

    fn address(&self) -> Address {
        todo!()
    }
}

let client = AraClient::builder()
    .signer(LedgerSigner { /* ... */ })
    .build_in_memory()
    .await?;
```

## Configuration

`AppConfig::default()` points to Sepolia testnet. Key fields:

```rust
let mut config = AppConfig::default();
config.ethereum.rpc_url = "https://your-rpc-url".to_string();
config.ethereum.chain_id = 11155111; // Sepolia

// Contract addresses (defaults to Sepolia deployment)
config.ethereum.ara_token_address = "0x...".to_string();
config.ethereum.staking_address = "0x...".to_string();
config.ethereum.registry_address = "0x...".to_string();
config.ethereum.marketplace_address = "0x...".to_string();
config.ethereum.collections_address = "0x...".to_string();
config.ethereum.name_registry_address = "0x...".to_string();
```

## Storage

The SDK uses SQLite for local state caching. Two builder options:

- `.build_in_memory()` — In-memory database (for testing/scripts)
- `.build()` — File-backed database at `config.storage.db_path`
- `.db_path("/custom/path.db")` — Override the DB path

The local database caches on-chain content, purchases, resale listings, reward history, and display names. Call `client.sync().sync_content()` and `client.sync().sync_rewards()` to populate it from chain events.

## Testing

```bash
# Unit tests (no network needed)
cargo test -p ara-sdk

# E2E tests on Sepolia (requires funded test wallets)
cargo test -p ara-sdk --test e2e_sepolia -- --ignored --nocapture --test-threads=1
```

## API Reference

Run `cargo doc -p ara-sdk --open` to generate full API documentation with examples.
