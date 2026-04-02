# Ara Marketplace — Architecture & Technical Reference

## Table of Contents

1. [System Overview](#1-system-overview)
2. [Smart Contracts](#2-smart-contracts)
3. [Purchase System](#3-purchase-system)
4. [Reward System](#4-reward-system)
5. [P2P and Peering System](#5-p2p-and-peering-system)
6. [Rust Backend (Tauri Commands)](#6-rust-backend-tauri-commands)
7. [Local Storage (SQLite)](#7-local-storage-sqlite)
8. [Frontend Architecture](#8-frontend-architecture)
9. [Data Flow Diagrams](#9-data-flow-diagrams)
10. [Security Model](#10-security-model)
11. [Configuration](#11-configuration)
12. [Development Guide](#12-development-guide)

---

## 1. System Overview

Ara is a desktop application that combines three systems:

1. **Ethereum smart contracts** — trustless registry, payments, and reward accounting
2. **iroh P2P network** — encrypted, content-addressed file transfer and seeder discovery
3. **Tauri desktop app** — native UI (React) over a Rust backend that bridges the other two

The architecture is deliberately layered. Smart contracts handle money and provenance. The P2P layer handles data. The app layer handles user experience. None of these layers trust each other blindly — the design is adversarial.

### Component Map

```
┌─────────────────────────────────────────────────────────────────┐
│                     Desktop App (Tauri v2)                       │
│                                                                  │
│  ┌──────────────────────┐    ┌──────────────────────────────┐   │
│  │  React Frontend      │    │  Rust Backend                │   │
│  │  (TypeScript)        │◄──►│  (Tauri Commands)            │   │
│  │                      │    │                              │   │
│  │  Pages:              │    │  Commands:                   │   │
│  │  • Marketplace       │    │  • content (publish/confirm) │   │
│  │  • ContentDetail     │    │  • marketplace (purchase)    │   │
│  │  • Publish           │    │  • seeding (start/stop)      │   │
│  │  • Library           │    │  • staking (stake/distribute)│   │
│  │  • Dashboard         │    │  • wallet (connect/balances) │   │
│  │  • Wallet            │    │  • sync (chain event sync)   │   │
│  └──────────────────────┘    └──────────┬───────────────────┘   │
│                                         │                        │
│                              ┌──────────▼───────────────────┐   │
│                              │  AppState (shared)           │   │
│                              │  • config (EthereumConfig)   │   │
│                              │  • db: Arc<Mutex<Database>>  │   │
│                              │  • iroh: Arc<Mutex<Option>>  │   │
│                              │  • gossip_tx: mpsc::Sender   │   │
│                              │  • wallet_address: Arc<Mutex>│   │
│                              └──────────┬───────────────────┘   │
└─────────────────────────────────────────┼───────────────────────┘
                                          │
              ┌───────────────────────────┼───────────────────────┐
              │                           │                       │
   ┌──────────▼──────────┐   ┌───────────▼────────┐   ┌─────────▼────────┐
   │  Ethereum (Sepolia) │   │  iroh P2P Network  │   │  SQLite (local)  │
   │                     │   │                    │   │                  │
   │  • AraStaking       │   │  • Blob store      │   │  • content       │
   │  • ContentRegistry  │   │  • iroh-gossip     │   │  • purchases     │
   │  • Marketplace      │   │  • QUIC transport  │   │  • seeding       │
   │  • MockARAToken     │   │  • Relay (NAT)     │   │  • delivery_rcpt │
   └─────────────────────┘   └────────────────────┘   └──────────────────┘
```

---

## 2. Smart Contracts

All contracts are on Sepolia testnet, compiled with Solidity 0.8.24, and tested with Foundry.

### 2.1 MockARAToken (`0xE8486e01aA1Da716448a3893792837AF9f1bBFa2`)

A standard ERC-20 token with a public `mint()` function for testnet distribution. In production this would be replaced with the real ARA token. 18 decimals. Used exclusively as the staking currency — ETH is used for payments.

### 2.2 AraStaking (`0x119554583bDB704CdA18f674054C2C7EF4C2A60c`)

Manages two types of stake:

**General stake** (`stakedBalance[user]`): ARA deposited via `stake(amount)`. Used to determine publisher eligibility. Anyone with ≥ 10 ARA staked (general) can publish content.

**Content stake** (`contentStake[user][contentId]`): Allocated from general stake via `stakeForContent(contentId, amount)`. Determines seeder eligibility for specific content. A seeder must have ≥ 1 ARA allocated to a specific `contentId` to be eligible for that content's reward pool.

```
stake(10 ARA) ─► stakedBalance[user] += 10
stakeForContent(contentId, 1) ─► stakedBalance[user] -= 1
                                  contentStake[user][contentId] += 1

isEligiblePublisher(user) = stakedBalance[user] >= 10 ARA
isEligibleSeeder(user, contentId) = contentStake[user][contentId] >= 1 ARA
```

Key functions:
- `stake(amount)` — transfer ARA in, increase general balance
- `unstake(amount)` — withdraw from general balance (cannot withdraw content-allocated stake)
- `stakeForContent(contentId, amount)` — move from general to content-specific
- `unstakeFromContent(contentId, amount)` — move content stake back to general
- `isEligiblePublisher(user)` — checks general balance
- `isEligibleSeeder(user, contentId)` — checks content-specific stake
- `getContentStake(user, contentId)` — view function

### 2.3 ContentRegistry (`0x2ECb7C21A99BcB52CD202a94484C935b31cB0Ea0`)

On-chain catalogue of all published content.

**Content struct:**
```solidity
struct Content {
    address creator;
    bytes32 contentHash;    // BLAKE3 hash from iroh — P2P identifier
    string metadataURI;     // JSON metadata (stored inline, not on IPFS in current impl)
    uint256 priceWei;       // Price buyers must pay (in wei)
    uint256 createdAt;
    bool active;
}
```

**Content ID generation:**
```solidity
contentId = keccak256(abi.encodePacked(contentHash, msg.sender, nonce))
```
The per-creator `nonce` means the same file can be published multiple times as distinct marketplace listings. The `contentId` is emitted in the `ContentPublished` event and extracted by the Rust confirm handler — it is NOT computable from the file alone.

**Events:**
- `ContentPublished(bytes32 indexed contentId, address indexed creator, bytes32 contentHash, string metadataURI, uint256 priceWei)`
- `ContentUpdated(bytes32 indexed contentId, uint256 newPriceWei, string newMetadataURI)`
- `ContentDelisted(bytes32 indexed contentId)`

Key functions:
- `publishContent(contentHash, metadataURI, priceWei)` — requires eligible publisher stake
- `updateContent(contentId, newPriceWei, newMetadataURI)` — creator only
- `delistContent(contentId)` — creator only, sets `active = false`
- `getCreator(contentId)`, `getPrice(contentId)`, `getContentHash(contentId)`, `isActive(contentId)` — view functions

### 2.4 Marketplace (`0xA4bBCCBFc6F7C12ad80c45C0aed386289636Bb6E`)

Handles ETH payments and seeder reward distribution.

**Payment split:**
```
purchase(contentId) {value: priceWei}
  → creator receives: price × creatorShareBps / 10000  (currently 85%)
  → rewardPool[contentId] += price × (10000 - creatorShareBps) / 10000  (currently 15%)
  → lastPurchaseTime[contentId] = block.timestamp
}
```

**Two reward distribution paths:**

#### Path 1: Creator Fast Path — `distributeRewards(contentId, seeders[], weights[])`

Callable by: content creator (via `registry.getCreator(contentId)`) OR the global `reporter` address.

No on-chain proof required. The caller provides seeder addresses and proportional weights. The contract verifies seeder eligibility (`staking.isEligibleSeeder`) and distributes proportionally:
```
amount[i] = rewardPool[contentId] × weights[i] / totalWeights
claimableRewards[seeders[i]] += amount[i]
rewardPool[contentId] -= distributed
```

#### Path 2: Trustless Fallback — `publicDistributeWithProofs(contentId, bundles[])`

Callable by: any eligible seeder, but only after `block.timestamp > lastPurchaseTime[contentId] + distributionWindow` (30 days by default).

Each `SeederBundle` contains buyer-signed `SignedReceipt` structs:
```solidity
struct SignedReceipt {
    uint256 timestamp;
    bytes signature;   // 65-byte EIP-712 ECDSA signature
}
struct SeederBundle {
    address seeder;
    SignedReceipt[] receipts;
}
```

For each receipt, the contract:
1. Computes EIP-712 hash: `keccak256("\x19\x01" || DOMAIN_SEPARATOR || keccak256(RECEIPT_TYPE_HASH || contentId || seeder || timestamp))`
2. Calls `ecrecover(hash, r, s, v)` to recover the buyer address
3. Checks `hasPurchased[contentId][buyer] == true`
4. Checks `usedReceipts[keccak256(contentId, seeder, buyer, timestamp)] == false`
5. Marks the receipt key as used
6. Increments `weights[i]`

After processing all bundles, distributes proportionally using the same `_distribute()` internal.

**EIP-712 domain:**
```
name: "AraMarketplace"
version: "1"
chainId: block.chainid (11155111 on Sepolia)
verifyingContract: address(this)
```

**EIP-712 type:**
```
DeliveryReceipt(bytes32 contentId,address seederEthAddress,uint256 timestamp)
```

**Claiming rewards:**
```solidity
function claimRewards() external {
    uint256 amount = claimableRewards[msg.sender];
    // ... transfer ETH to msg.sender
}
```

**State variables:**
- `hasPurchased[contentId][buyer]` — purchase ledger
- `rewardPool[contentId]` — undistributed ETH per content
- `claimableRewards[seeder]` — ETH ready to withdraw per seeder
- `lastPurchaseTime[contentId]` — for distribution window
- `distributionWindow` — default 30 days, owner-configurable
- `usedReceipts[key]` — replay protection for public distribution
- `DOMAIN_SEPARATOR` — immutable EIP-712 domain hash
- `RECEIPT_TYPE_HASH` — public constant for frontend use

---

## 3. Purchase System

### 3.1 Full Purchase Flow

```
User clicks "Purchase for X ETH"
        │
        ▼
[Rust] purchase_content(contentId)
  • Check wallet connected
  • Query DB for price, title
  • Check on-chain has_purchased (idempotency)
  • Encode Marketplace.purchase(contentId) calldata
  • Return TransactionRequest { to, data, value: priceWei }
        │
        ▼
[Frontend] signAndSendTransactions(walletProvider, [tx])
  • walletProvider.request({ method: "eth_sendTransaction", params })
  • MetaMask prompts user for ETH approval
  • Returns txHash
        │
        ▼
[Rust] confirm_purchase(contentId, txHash)
  • INSERT INTO purchases (content_id, buyer, price_paid_wei, tx_hash, purchased_at)
  • Look up content_hash, publisher_node_id, publisher_relay_url, filename from DB
  • Build NodeAddr with relay URL for publisher
  • ContentManager.download_from(contentHash, publisherAddr) — iroh blob fetch
  • ContentManager.export_blob(contentHash, outputPath) — write to downloads/
  • Detect file type from magic bytes if no extension
  • UPDATE purchases SET downloaded_path = outputPath
  • INSERT INTO seeding (content_id, active=1, ...)  — auto-start seeding
  • GossipCmd::AnnounceSeeding { content_hash, bootstrap: [publisherNodeId] }
        │
        ▼
[Frontend] (optional) Sign EIP-712 delivery receipt
  • Show "Sign Receipt" prompt in success banner
  • walletProvider.request({ method: "eth_signTypedData_v4", params })
  • broadcast_delivery_receipt(contentId, seederEthAddr, buyerEthAddr, sig, timestamp)
        │
        ▼
[Rust] broadcast_delivery_receipt(...)
  • INSERT INTO delivery_receipts
  • Query content_hash for gossip topic
  • GossipCmd::BroadcastDeliveryReceipt { ... }
  • GossipActor broadcasts GossipMessage::DeliveryReceipt on content topic
```

### 3.2 Download Details

The download uses iroh's blob protocol. The buyer node connects to the publisher's iroh node using:
- Publisher's `NodeId` (Ed25519 public key, stored in `content.publisher_node_id`)
- Publisher's relay URL (stored in `content.publisher_relay_url`, used for NAT traversal)

Without a relay URL, cross-NAT connections fail silently. The relay URL is always written at publish time and always used at download time.

If the buyer IS the publisher (same `node_id`), the blob is already in the local iroh store — download is skipped.

After download, the buyer's node is added to the gossip swarm for that content as a seeder. This is automatic — anyone who has downloaded content begins seeding it immediately (they can stop via the Library toggle).

### 3.3 Idempotency

If the purchase transaction confirmed on-chain but `confirm_purchase` was never called (e.g., app crashed), the next `purchase_content` call detects the on-chain purchase via `has_purchased()` and returns an empty `transactions` array. `confirm_purchase` is then called with a dummy hash `"0x0"` to record the purchase locally and trigger the download.

---

## 4. Reward System

### 4.1 Overview

The reward system solves three problems simultaneously:

1. **Revenue share**: Content seeders should earn a portion of purchase revenue
2. **Anti-fraud**: Seeders can't inflate their own reward share
3. **Liveness**: A non-cooperative creator can't lock rewards forever

### 4.1b Free Content & Tipping

**Free content** (`MIN_PRICE = 0`): Creators can publish at price 0. Free content requires no on-chain purchase to download — buyers get the file via P2P directly. The 10 ARA staking requirement still applies to publish free content.

**Tipping** (`Marketplace.tipContent(contentId)`): Anyone can tip ETH on any content (free or paid). Tips use the same 85/2.5/12.5 split as purchases:
- Creator gets 85% (via `_payCreatorETH`, supports collaborator splits)
- Stakers get 2.5% (via `staking.addReward`)
- Seeders get 12.5% (stored in `buyerReward[contentId][tipper]`)

Key differences from purchases:
- Tips do NOT mint edition tokens (tipping ≠ ownership)
- Tips are additive: `buyerReward[contentId][tipper] += rewardAmount`
- Multiple tips from the same address accumulate
- Protected by `nonReentrant` (same as purchase)
- Seeders claim tip rewards via the same delivery receipt mechanism

### 4.2 Delivery Receipts

The anti-fraud mechanism is **buyer-signed EIP-712 delivery receipts**. After downloading content, the buyer is prompted (optionally) to sign:

```typescript
const typedData = {
  types: {
    DeliveryReceipt: [
      { name: "contentId", type: "bytes32" },
      { name: "seederEthAddress", type: "address" },
      { name: "timestamp", type: "uint256" },
    ],
  },
  primaryType: "DeliveryReceipt",
  domain: {
    name: "AraMarketplace",
    version: "1",
    chainId: 11155111,
    verifyingContract: marketplaceAddress,
  },
  message: {
    contentId: content.content_id,       // 0x-prefixed hex bytes32
    seederEthAddress: content.creator,   // seeder's Ethereum address
    timestamp: Math.floor(Date.now() / 1000),
  },
};
const signature = await walletProvider.request({
  method: "eth_signTypedData_v4",
  params: [buyerAddress, JSON.stringify(typedData)],
});
```

This signature proves:
- A specific buyer (verified by `hasPurchased` on-chain)
- Received content from a specific seeder (seeder's ETH address in the message)
- At a specific time (replay protection via timestamp)

**Why buyers can't be tricked into signing false receipts**: The receipt only claims delivery occurred. It doesn't authorize any fund transfer. There is no economic incentive for a buyer to sign a false receipt attributing delivery to a seeder who didn't serve them — it just costs them a MetaMask interaction.

**Why seeders can't forge receipts**: The signature is made with the buyer's private key. Seeders don't have buyer private keys.

### 4.3 Receipt Storage and Broadcast

After signing, `broadcast_delivery_receipt` is called:

1. `INSERT INTO delivery_receipts (content_id, seeder_eth_address, buyer_eth_address, signature, timestamp)` — stored locally
2. Look up BLAKE3 content hash from `content` table (the gossip topic key)
3. Send `GossipCmd::BroadcastDeliveryReceipt` to the gossip actor
4. Gossip actor broadcasts `GossipMessage::DeliveryReceipt` on the content's gossip topic

All nodes subscribed to that content's gossip topic receive the receipt:
- **Seeder nodes** store it in their local `delivery_receipts` table
- **Creator nodes** store it in their `delivery_receipts` table
- Both can later use these receipts for reward distribution

### 4.4 Creator Fast Path Distribution

The creator opens the Library → Published tab and clicks "Distribute" for a content item. This calls `prepare_distribute_rewards(contentId)`:

```
1. Read all delivery_receipts for contentId from DB
2. For each receipt:
   a. Compute EIP-712 hash (same as the buyer signed)
   b. Recover buyer address via ECDSA ecrecover
   c. Verify recovered address == stored buyer_eth_address (signature validity)
   d. Query chain: marketplace.has_purchased(contentId, buyer) (fraud check)
   e. If valid: increment seeder_receipt_counts[seeder_eth_address]
3. For each seeder with valid receipts:
   a. Query chain: staking.is_eligible_seeder(seeder, contentId)
   b. Query chain: staking.content_stake(seeder, contentId)
   c. weight = receipt_count × content_stake (or just receipt_count if stake = 0)
4. Encode distributeRewards(contentId, seeders[], weights[]) calldata
5. Return TransactionRequest for creator to sign via MetaMask
```

After the transaction confirms on-chain:
- `claimableRewards[seeder]` increases for each eligible seeder
- `rewardPool[contentId]` decreases by the distributed amount
- `RewardsDistributed` event emitted

### 4.5 Trustless Fallback Distribution

If the creator has not distributed rewards within 30 days of the last purchase, any eligible seeder can call `prepare_public_distribute(contentId)`:

```
1. Query chain: marketplace.last_purchase_time(contentId)
2. Query chain: marketplace.distribution_window()
3. Check: now > last_purchase_time + distribution_window (else error with time remaining)
4. Read delivery_receipts from DB
5. Build SeederBundle[] with all receipts, grouped by seeder
6. Encode publicDistributeWithProofs(contentId, bundles[]) calldata
7. Return TransactionRequest for seeder to sign via MetaMask
```

The contract verifies everything on-chain. The calling seeder bears the gas cost but earns their proportional share from the pool.

### 4.6 Claiming Rewards

Seeders view their claimable rewards on the Wallet page (via `get_stake_info` → `marketplace.claimableRewards(seeder)`). The "Claim Rewards" button calls `claim_rewards` which encodes `claimRewards()` calldata. MetaMask signs the transaction, and ETH is transferred to the seeder's wallet.

### 4.7 NodeId → ETH Address Mapping

For receipt verification, we need to know a seeder's Ethereum address (to check `contentStake`). Seeders are identified on the P2P network by their iroh `NodeId` (an Ed25519 public key). These are different identity systems.

The `SeederIdentity` gossip message links them:
```rust
GossipMessage::SeederIdentity {
    node_id: [u8; 32],      // iroh NodeId bytes
    eth_address: [u8; 20],  // Ethereum address
    signature: Vec<u8>,     // Ed25519 signature proving eth_address ownership
}
```

Received `SeederIdentity` messages are stored in `content_seeders.eth_address`. When preparing distribution, the ETH address is looked up by node ID from this table.

*Note: The `BroadcastSeederIdentity` gossip command is implemented but not yet called from `start_seeding` — this is pending work.*

---

## 5. P2P and Peering System

### 5.1 iroh Overview

[iroh](https://iroh.computer/) is a Rust library for peer-to-peer networking:
- **Blob store**: content-addressed storage using BLAKE3 hashes
- **QUIC transport**: encrypted, multiplexed connections
- **Relay network**: NAT traversal via relay servers (like TURN for WebRTC)
- **iroh-gossip**: pubsub over the same transport

Each Ara node is an iroh node with:
- A unique `NodeId` (Ed25519 keypair, persisted in `data/iroh/`)
- A local blob store (persisted in `data/iroh/`)
- Connections to other iroh nodes via QUIC (direct) or relay (NAT-traversed)

### 5.2 Content Storage

When a creator publishes:
```
File → BLAKE3 hash → iroh blob store (add_bytes)
     → metadataURI = JSON embedding title/description/type/size
     → ContentRegistry.publishContent(blake3Hash, metadataURI, priceWei) tx
     → ContentPublished event: contentId = keccak256(blake3Hash, creator, nonce)
     → DB: INSERT INTO content (content_id, content_hash=blake3, publisher_node_id, ...)
```

The BLAKE3 hash (`content_hash`) is the iroh blob identifier. The keccak256 `content_id` is the on-chain Marketplace identifier. These are stored separately in the DB and must never be confused.

### 5.3 Content Transfer

When a buyer purchases:
```
confirm_purchase(contentId, txHash)
  → Look up content_hash (BLAKE3) + publisher_node_id + publisher_relay_url from DB
  → Build NodeAddr { node_id: publisher_id, relay_url }
  → iroh_node.blobs().download_with_opts(hash, NodeAddr).await
  → Blob stored locally in iroh store
  → Export to downloads/ directory
```

The `publisher_relay_url` is critical for NAT traversal. Without it, the buyer can only connect to the publisher if they share a network or the publisher has a public IP.

For buyer→seeder (non-publisher) downloads, the seeder's NodeId and relay URL would need to come from the `content_seeders` table or gossip. Currently, buyers always download from the publisher node. Seeder-to-buyer direct transfer is a planned enhancement.

### 5.4 Gossip-Based Seeder Discovery

Each content hash has a dedicated iroh-gossip **topic**:
```rust
pub fn topic_for_content(hash: &ContentHash) -> TopicId {
    TopicId::from(*hash)  // use BLAKE3 hash as topic ID directly
}
```

The `GossipActor` manages all topic subscriptions in a background task. It exposes a channel `mpsc::Sender<GossipCmd>` because `GossipReceiver` is `!Sync` and cannot live in `AppState`.

**Message flow — seeder joins:**
```
start_seeding(contentId)
  → Look up content_hash from DB
  → Look up known seeders (content_seeders) for bootstrap peers
  → GossipCmd::AnnounceSeeding { content_hash, bootstrap: [knownPeerIds] }
  → GossipActor joins iroh-gossip topic for content_hash
  → GossipActor broadcasts GossipMessage::SeederAnnounce { content_hash, node_id_bytes }
  → Peers receive announce, add to known_seeders map
  → GossipActor's recv_loop: NeighborUp → re-broadcast announce (so late joiners discover us)
```

**Message flow — seeder leaves:**
```
stop_seeding(contentId)
  → GossipCmd::LeaveSeeding { content_hash }
  → GossipActor broadcasts GossipMessage::SeederLeave
  → Removes topic from active_topics map
```

**Periodic re-announce:**
Every 60 seconds, the GossipActor re-broadcasts `SeederAnnounce` on all active topics. This ensures nodes that bootstrap into the gossip overlay after initial announce still discover existing seeders.

**Persistence:**
Discovered seeder `NodeId`s are persisted to `content_seeders` table on discovery. On restart, these are loaded and used as bootstrap peers when rejoining topics, ensuring the gossip overlay reconnects quickly.

### 5.5 GossipActor Implementation

```
mpsc channel (tx → GossipCmd)  ───►  GossipActor (tokio task)
                                         │
                        ┌────────────────┼───────────────────────────┐
                        │                │                           │
              ┌─────────▼──────┐  ┌──────▼──────────┐  ┌───────────▼───────┐
              │  active_topics │  │  known_seeders  │  │  recv_loop tasks  │
              │  (HashMap of   │  │  (Arc<Mutex<    │  │  (one per topic,  │
              │  GossipSender) │  │  HashMap>>)     │  │  !Sync confined)  │
              └────────────────┘  └─────────────────┘  └───────────────────┘
```

`GossipCmd` variants:
- `AnnounceSeeding { content_hash, bootstrap }` — join topic + broadcast announce
- `LeaveSeeding { content_hash }` — broadcast leave + drop sender
- `BroadcastDeliveryReceipt { content_hash, content_id, seeder_eth_address, buyer_eth_address, signature, timestamp }` — broadcast receipt on topic
- `BroadcastSeederIdentity { node_id, eth_address, signature }` — broadcast identity on all topics (pending: not yet called)

`RecvEvent` (internal channel, recv_loop → actor):
- `NeighborUp { content_hash }` — re-broadcast announce
- `PeerChanged` — emit `seeder-stats-updated` Tauri event to frontend
- `SeederPersist { content_hash, node_id }` — persist to DB
- `DeliveryReceiptReceived { ... }` — store in delivery_receipts DB
- `SeederIdentityReceived { ... }` — update content_seeders.eth_address

### 5.6 Bytes Served Tracking

The `blob_events.rs` module monitors iroh's blob transfer events to track how many bytes each seeder has served. It subscribes to `GetRequestReceived` and `TransferCompleted` events from the iroh blob engine, pairing request IDs to accumulate actual bytes transferred.

This data populates `seeding.bytes_served` in the DB and is surfaced on the Dashboard.

---

## 6. Rust Backend (Tauri Commands)

### 6.1 AppState

```rust
pub struct AppState {
    pub config: AppConfig,                    // loaded from config file at startup
    pub db: Arc<Mutex<Database>>,             // SQLite, single connection
    pub iroh_node: Arc<Mutex<Option<Node>>>,  // lazy-initialized on first use
    pub gossip_tx: Mutex<Option<mpsc::Sender<GossipCmd>>>,  // set when iroh starts
    pub wallet_address: Arc<Mutex<Option<String>>>,
    pub known_seeders: KnownSeeders,          // Arc<Mutex<HashMap<ContentHash, HashSet<NodeId>>>>
}
```

`chain_client()` creates a new `ChainClient` on each call — HTTP providers are cheap and stateless, so there's no need to pool them.

`ensure_iroh()` lazy-initializes the iroh node on first use, creating the gossip actor and blob event listener as side effects.

### 6.2 Command: publish_content

```
1. Validate wallet connected, publisher eligible (staking.is_eligible_publisher on-chain)
2. Read file bytes → add to iroh blob store → get BLAKE3 hash
3. Build metadataURI JSON string (title, description, type, size, filename, publisher node info)
4. Parse price ETH → wei
5. Encode ContentRegistry.publishContent(hash, metadataURI, priceWei) calldata
6. Return TransactionRequest
```

### 6.3 Command: confirm_publish

```
1. Find pending content by content_hash in DB (active=0)
2. Extract contentId from ContentPublished event in tx receipt
3. UPDATE content SET content_id = contentId, active = 1
4. GossipCmd::AnnounceSeeding (publisher seeds their own content immediately)
```

### 6.4 Command: sync_content

Reads `ContentPublished`, `ContentUpdated`, and `ContentDelisted` events from the chain using a stored `last_sync_block` cursor (from `config` table). For each new `ContentPublished` event, decodes the `metadataURI` JSON and calls `db.upsert_synced_content(...)`. This keeps the local DB in sync with on-chain listings without a centralized indexer.

### 6.5 Chain Client Architecture

`ara-chain` provides typed wrappers over alloy's `sol!` macro:

```rust
pub struct ChainClient {
    pub token: TokenClient<RootProvider>,
    pub staking: StakingClient<RootProvider>,
    pub registry: RegistryClient<RootProvider>,
    pub marketplace: MarketplaceClient<RootProvider>,
}
```

Each client wraps an alloy `sol!`-generated contract bindings instance. Read operations use `.call().await`. Write operations encode ABI calldata via `SolCall::abi_encode()` and return raw bytes to the Rust command layer, which packages them into `TransactionRequest` for the frontend to sign.

### 6.6 EIP-712 Verification in Rust

`prepare_distribute_rewards` verifies receipt signatures without going on-chain:

```rust
fn compute_domain_separator(marketplace_addr: Address, chain_id: u64) -> [u8; 32] {
    // keccak256(abi.encode(DOMAIN_TYPE_HASH, name, version, chainId, verifyingContract))
}

fn verify_receipt_signature(
    domain_separator: &[u8; 32],
    content_id: FixedBytes<32>,
    seeder: Address,
    timestamp: u64,
    signature_hex: &str,
) -> Option<Address> {
    // Compute struct hash: keccak256(RECEIPT_TYPE_HASH, contentId, seeder, timestamp)
    // Compute EIP-712 hash: keccak256("\x19\x01" || domain_separator || struct_hash)
    // Parse 65-byte signature (r, s, v)
    // alloy::primitives::Signature::from_raw_parts(r, s, v)
    // sig.recover_address_from_prehash(&hash)
}
```

---

## 7. Local Storage (SQLite)

The database is opened at startup by `Database::open(path)` which runs all migrations. Migrations use `CREATE TABLE IF NOT EXISTS` for idempotency, plus `ALTER TABLE ADD COLUMN` with ignored errors for incremental column additions.

### Schema

**`content`** — All known content (synced from chain + locally published)
```sql
content_id TEXT PRIMARY KEY,    -- keccak256 on-chain ID (0x-prefixed hex)
content_hash TEXT NOT NULL,     -- BLAKE3 hash (0x-prefixed hex) — iroh identifier
creator TEXT NOT NULL,          -- Ethereum address
metadata_uri TEXT NOT NULL,     -- JSON string (inline) or URI
price_wei TEXT NOT NULL,        -- Wei as decimal string
title TEXT,
description TEXT,
content_type TEXT,              -- "music", "video", "document", "software", "game"
thumbnail_url TEXT,
file_size_bytes INTEGER,
active INTEGER NOT NULL,        -- 1 = listed, 0 = delisted or pending confirm
created_at INTEGER NOT NULL,    -- Unix timestamp
publisher_node_id TEXT,         -- iroh NodeId (base32 string)
publisher_relay_url TEXT,       -- e.g. "https://relay.iroh.network"
filename TEXT                   -- original filename with extension
```

**`purchases`** — Content purchased by connected wallet
```sql
content_id TEXT NOT NULL,
buyer TEXT NOT NULL,            -- Ethereum address
price_paid_wei TEXT NOT NULL,
tx_hash TEXT,                   -- null for local-only / already-purchased cases
purchased_at INTEGER NOT NULL,
downloaded_path TEXT,           -- local filesystem path after download
PRIMARY KEY (content_id, buyer)
```

**`seeding`** — Active and historical seeding sessions
```sql
content_id TEXT PRIMARY KEY,
active INTEGER NOT NULL,        -- 1 = currently seeding
bytes_served INTEGER NOT NULL,
peer_count INTEGER NOT NULL,
started_at INTEGER NOT NULL
```

**`delivery_receipts`** — Buyer-signed EIP-712 receipts collected from gossip
```sql
content_id TEXT NOT NULL,           -- keccak256 content ID (for on-chain use)
seeder_eth_address TEXT NOT NULL,   -- checksummed Ethereum address
buyer_eth_address TEXT NOT NULL,    -- checksummed Ethereum address
signature TEXT NOT NULL,            -- 0x-prefixed hex, 65 bytes
timestamp INTEGER NOT NULL,         -- Unix timestamp from the signed message
PRIMARY KEY (content_id, seeder_eth_address, buyer_eth_address)
-- One receipt per buyer-seeder pair. Newer receipt for same pair is ignored (IGNORE).
```

**`content_seeders`** — P2P seeder nodes discovered via gossip
```sql
content_hash TEXT NOT NULL,     -- BLAKE3 hash (gossip topic key)
node_id TEXT NOT NULL,          -- iroh NodeId string
eth_address TEXT,               -- Ethereum address (from SeederIdentity message)
discovered_at INTEGER NOT NULL,
PRIMARY KEY (content_hash, node_id)
```

**`config`** — Key-value store for persistent app state
```sql
key TEXT PRIMARY KEY,
value TEXT NOT NULL
-- Used for: last_sync_block, any other persistent settings
```

---

## 8. Frontend Architecture

### 8.1 Stack

- React 18, TypeScript
- React Router v6 (client-side routing within Tauri webview)
- Zustand (wallet state)
- Tailwind CSS v3
- Web3Modal v3 + ethers v6 (wallet connection and signing)
- Tauri v2 API (`@tauri-apps/api`) for IPC

### 8.2 IPC Pattern

All Rust backend calls go through typed wrappers in `app/src/lib/tauri.ts`:

```typescript
import { invoke } from "@tauri-apps/api/core";

export async function purchaseContent(contentId: string): Promise<PurchasePrepareResult> {
  return invoke("purchase_content", { contentId });
}
```

Tauri v2 automatically converts camelCase TypeScript parameter names to snake_case Rust parameter names.

### 8.3 Transaction Signing

All on-chain transactions follow the same pattern in `app/src/lib/transactions.ts`:

```typescript
async function signAndSendTransactions(
  walletProvider: Eip1193Provider,
  requests: TransactionRequest[],
  onStatus?: (msg: string) => void
): Promise<string>
```

Transactions are sent via raw EIP-1193 `eth_sendTransaction` requests (not ethers.js `BrowserProvider`). This avoids the `eth_blockNumber` pre-flight call that WalletConnect doesn't proxy, which would cause silent failures with hardware wallets.

Multiple transactions in a sequence are signed one at a time. The last `txHash` is returned.

### 8.4 Pages

**Marketplace** (`/`): Lists all content from local DB (populated by `sync_content`). Shows title, type icon, price, seeder count. Clicking navigates to ContentDetail.

**ContentDetail** (`/content/:contentId`): Shows full content metadata. Creator view: edit button. Buyer view: Purchase button → EIP-712 receipt prompt. If already purchased: shows download path and seeding status.

**Publish** (`/publish`): File picker → title/description/type/price form → `publish_content` → MetaMask → `confirm_publish`. Shows iroh node ID and relay URL for the user.

**Library** (`/library`): Two tabs:
- *Purchased*: All purchased items. Per-item: Open File, Folder, Seed toggle.
- *Published*: All published items. Per-item: delivery count, reward pool balance, Distribute button, Seed toggle, Delist button.

**Dashboard** (`/dashboard`): Seeding statistics from `get_seeder_stats`. Shows bytes_served, peer_count, stake per content.

**Wallet** (`/wallet`): ETH balance, ARA balance, staked ARA, claimable rewards. Stake/Unstake inputs. Claim Rewards button.

### 8.5 Wallet State (Zustand)

`walletStore.ts` manages:
- Connection state (`address`, `isConnecting`)
- Balances (`ethBalance`, `araBalance`, `araStaked`, `claimableRewards`)
- Transaction state (`isSendingTx`, `txStatus`, `error`)
- Actions: `onWalletConnected`, `onWalletDisconnected`, `refreshBalances`, `stakeAra`, `unstakeAra`, `claimRewards`

Web3Modal `useWeb3ModalAccount()` hooks trigger `onWalletConnected`/`onWalletDisconnected` which call the Rust `connect_wallet`/`disconnect_wallet` commands to sync the wallet address into AppState.

---

## 9. Data Flow Diagrams

### 9.1 Publish Flow

```
Creator                  App (Rust)              iroh             Ethereum
  │                          │                    │                   │
  │── File → Publish form ──►│                    │                   │
  │                          │── add_bytes ───────►│                   │
  │                          │◄─ content_hash ─────│                   │
  │                          │                    │                   │
  │◄─ TransactionRequest ────│                    │                   │
  │                          │                    │                   │
  │── MetaMask sign ─────────────────────────────────────────────────►│
  │◄─ txHash ─────────────────────────────────────────────────────────│
  │                          │                    │                   │
  │── confirm_publish ──────►│                    │                   │
  │                          │── get tx receipt ─────────────────────►│
  │                          │◄─ ContentPublished(contentId) ─────────│
  │                          │── UPDATE content SET active=1          │
  │                          │── GossipCmd::AnnounceSeeding ──────────────────►
  │◄─ Done ──────────────────│                    │                   │
```

### 9.2 Purchase + Download Flow

```
Buyer                    App (Rust)              iroh      Ethereum    Gossip
  │                          │                    │            │          │
  │── purchase_content ─────►│                    │            │          │
  │                          │── hasPurchased ───────────────►│          │
  │◄─ TransactionRequest ────│                    │            │          │
  │── MetaMask sign ─────────────────────────────────────────►│          │
  │◄─ txHash ────────────────────────────────────────────────►│          │
  │── confirm_purchase ─────►│                    │            │          │
  │                          │── INSERT purchases  │            │          │
  │                          │── download_from(publisherAddr) ─►│         │
  │                          │◄─ blob received ───────────────  │         │
  │                          │── export_blob ─────►│            │          │
  │                          │── INSERT seeding    │            │          │
  │                          │── AnnounceSeeding ─────────────────────────►│
  │◄─ Done ──────────────────│                    │            │          │
  │                          │                    │            │          │
  │── (optional) Sign EIP-712 receipt             │            │          │
  │── broadcast_delivery_receipt ───────────────────────────────────────►│
  │                          │                    │            │  (stored by seeder + creator)
```

### 9.3 Reward Distribution Flow (Creator Fast Path)

```
Creator                  App (Rust)           Chain           Seeders
  │                          │                  │                │
  │── "Distribute" click ───►│                  │                │
  │                          │── DB: read delivery_receipts      │
  │                          │── ECDSA verify each signature     │
  │                          │── hasPurchased per buyer ────────►│
  │                          │── isEligibleSeeder per seeder ───►│
  │                          │── getContentStake per seeder ─────►│
  │                          │── compute weights                 │
  │◄─ TransactionRequest ────│                  │                │
  │── MetaMask sign ─────────────────────────────────────────────►│
  │                          │                  │                │
  │                          │     claimableRewards[seeder] += share
  │                          │     rewardPool[contentId] -= total
  │                          │                  │                │
  │                          │                  │──── seeder clicks "Claim Rewards"
  │                          │                  │──── ETH transfer to seeder wallet
```

---

## 10. Security Model

### 10.1 Trust Assumptions

| Component | Trusted? | Notes |
|-----------|---------|-------|
| Ethereum | Yes | L1 consensus; assuming Sepolia finality |
| Smart contract code | Yes | Open-source, immutable after deploy |
| iroh network | Partially | Content is hash-verified; relay servers can see IP addresses |
| Buyer's wallet | Yes | Signs receipts with their private key |
| Creator's node | No | Cannot steal reward pool (belongs to contract) |
| Seeder's node | No | Cannot forge buyer signatures |
| App code | Partially | Open-source; Tauri prevents frontend from accessing OS directly |

### 10.2 Attack Vectors and Mitigations

**Fake delivery receipts**: A seeder wants to claim they served content they didn't.
- *Mitigation*: Receipts require a buyer's ECDSA signature. Seeders don't have buyer private keys. `ecrecover` on-chain verifies authenticity.

**Receipt replay**: Submit the same valid receipt twice.
- *Mitigation*: `usedReceipts[keccak256(contentId, seeder, buyer, timestamp)]` is set to `true` on first use in `publicDistributeWithProofs`. Timestamps add an additional differentiator; the composite key includes all four fields.

**Creator reward hoarding**: Creator never calls `distributeRewards`, keeping pool locked.
- *Mitigation*: After `distributionWindow` (30 days), any eligible seeder can call `publicDistributeWithProofs` without creator cooperation.

**Fake purchases for receipt eligibility**: Create fake on-chain purchases to make illegitimate receipts appear valid.
- *Mitigation*: Fake purchases require paying the full content price to the creator. The attacker can't profit — they spend ETH to create a verifiable receipt, but the receipt only earns a proportional share of the 15% pool, which is less than the purchase cost.

**Sybil seeder attack**: Register many seeder identities to claim a larger pool share.
- *Mitigation*: Each seeder must stake ≥ 1 ARA per content to be eligible. Reward weight = receipt_count × content_stake. More stake per seeder increases their share, but staking costs real ARA. Thin Sybil identities with minimal stake earn minimally.

**Gossip spam**: Flood gossip topics with fake messages.
- *Mitigation*: iroh-gossip operates within a connected subgraph; nodes that misbehave can be dropped. Receipt verification happens at storage/use time, not receipt time. Invalid signatures are discarded during `prepare_distribute_rewards`.

### 10.3 What Cannot Be Manipulated

- **Payment splits**: Hardcoded in contract; 85%/15% division happens atomically with purchase.
- **Content IDs**: `keccak256(contentHash, creator, nonce)` — collision-resistant; no two different inputs produce the same ID.
- **Who can distribute**: Only creator or global reporter (fast path), or any eligible seeder after window (fallback).
- **Who can claim**: Only the address with `claimableRewards > 0`.

---

## 11. Configuration

### 11.1 AppConfig (Rust)

Located at `data/ara-config.json` (created with defaults on first run):

```json
{
  "ethereum": {
    "rpc_url": "https://ethereum-sepolia.publicnode.com",
    "chain_id": 11155111,
    "ara_token_address": "0xE8486e01aA1Da716448a3893792837AF9f1bBFa2",
    "staking_address": "0x119554583bDB704CdA18f674054C2C7EF4C2A60c",
    "registry_address": "0x2ECb7C21A99BcB52CD202a94484C935b31cB0Ea0",
    "marketplace_address": "0xA4bBCCBFc6F7C12ad80c45C0aed386289636Bb6E",
    "deployment_block": 10293374
  },
  "iroh": {
    "relay_urls": ["https://relay.iroh.network"],
    "data_dir": "data/iroh"
  },
  "storage": {
    "db_path": "data/ara-marketplace.db",
    "downloads_dir": "downloads"
  }
}
```

After contract redeployment, update `*_address` fields and `deployment_block`.

### 11.2 Frontend Environment

`app/.env`:
```
VITE_WALLETCONNECT_PROJECT_ID=your_project_id
```

Get a project ID at [cloud.walletconnect.com](https://cloud.walletconnect.com).

---

## 12. Development Guide

### 12.1 Adding a New Tauri Command

1. Add the function to the appropriate file in `app/src-tauri/src/commands/`
2. Register it in `app/src-tauri/src/lib.rs` `invoke_handler`
3. Add the typed TypeScript wrapper to `app/src/lib/tauri.ts`
4. Use from frontend

### 12.2 Adding a New DB Column

```rust
// In storage.rs migrate():
let _ = self.conn.execute("ALTER TABLE tablename ADD COLUMN new_col TEXT", []);
// The `let _` ignores the "duplicate column" error on existing databases
```

### 12.3 Adding a New Gossip Message Type

1. Add variant to `GossipMessage` enum in `crates/ara-p2p/src/discovery.rs`
   - Use `Vec<u8>` for any byte arrays > 32 bytes (serde only supports `[u8; N]` for N ≤ 32)
2. Add handler in `gossip_actor.rs` `recv_loop` and `run` (new `GossipCmd` variant if needed)
3. Add any DB storage in the `RecvEvent` handler

### 12.4 Deploying Updated Contracts

```bash
cd contracts
forge script script/Deploy.s.sol \
  --rpc-url $SEPOLIA_RPC_URL \
  --private-key $DEPLOYER_PRIVATE_KEY \
  --broadcast \
  --verify \
  --etherscan-api-key $ETHERSCAN_API_KEY

# Update addresses in:
# 1. crates/ara-core/src/config.rs (AppConfig::default)
# 2. app/src/lib/types.ts CONTRACTS (if used)
# 3. CLAUDE.md contract address table
# 4. README.md contract address table
# 5. docs/ARCHITECTURE.md contract address table
```

### 12.5 Running Tests

```bash
# Smart contract tests (Foundry)
cd contracts && forge test -vvv

# Rust unit + integration tests
cargo test --workspace

# Note: test_two_node_transfer is flaky (iroh relay timing)
# Run multiple times or add RUST_LOG=iroh=debug for diagnostics
```

### 12.6 Common Debugging

**No content appears in marketplace**: Run `sync_content` from the frontend (or check `last_sync_block` in the `config` DB table — reset to `deployment_block` to re-sync from scratch).

**Download fails with connection error**: Check `publisher_relay_url` is populated in the `content` table. A bare NodeId without relay URL cannot cross NAT.

**Seeder stats show 0 bytes**: The blob events monitor may not have started if iroh wasn't initialized before seeding. Check that `ensure_iroh()` succeeded and the gossip actor started.

**distributeRewards fails with "no eligible seeders"**: Seeders must have called `stakeForContent` for the specific contentId (not just general staking). Check `contentStake[seeder][contentId]` on-chain.

**Receipt count shows 0 despite purchases**: Buyers must explicitly sign and broadcast delivery receipts. This is an optional step in the current UI. Until `SeederIdentity` broadcasts are wired to `start_seeding`, the seeder ETH address lookup from gossip is also pending.
