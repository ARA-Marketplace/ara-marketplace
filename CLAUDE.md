# Ara Marketplace - Technical Reference

## What This Is

A decentralized content marketplace desktop app. Creators publish files, buyers purchase with ETH, seeders distribute content peer-to-peer and earn rewards. Think "BitTorrent meets an app store, with Ethereum payments and cryptographic anti-fraud."

## Stack

- **Desktop app**: Tauri v2 (Rust backend + React/TypeScript frontend)
- **Smart contracts**: Solidity 0.8.24, built/tested with Foundry (forge)
- **P2P layer**: iroh (Rust) for content transfer, iroh-gossip for seeder discovery
- **Ethereum**: alloy 1.x (Rust), ethers v6 (TypeScript), Web3Modal v3 for wallet connection
- **Storage**: SQLite (rusqlite) for local state, iroh blob store for content

## Project Layout

```
ara-marketplace/
  contracts/          # Solidity smart contracts (Foundry project)
    src/              # AraStaking, AraContent, Marketplace, MockARAToken
    test/             # Forge tests (73+ tests)
    script/           # Deploy.s.sol
  crates/
    ara-core/         # Config (AppConfig), SQLite storage (Database), shared types
    ara-p2p/          # iroh node, content management, discovery, seeding, metrics
    ara-chain/        # Typed Ethereum clients (token, staking, registry, marketplace, events)
  app/
    src-tauri/src/    # Tauri commands (Rust backend)
      commands/       # content, marketplace, seeding, staking, tx, wallet, sync, types
      state.rs        # AppState (config, DB, iroh node, gossip sender, wallet_address)
      setup.rs        # App initialization (loads config, opens DB, lazy-inits iroh)
      gossip_actor.rs # Background gossip task (mpsc channel pattern)
      blob_events.rs  # iroh blob transfer event monitoring (bytes_served tracking)
    src/              # React frontend
      pages/          # Marketplace, Publish, ContentDetail, Library, Dashboard, Wallet
      components/     # Navbar, Layout
      lib/            # tauri.ts (IPC bindings), transactions.ts, web3modal.ts, types.ts
      store/          # walletStore.ts (Zustand)
  docs/
    ARCHITECTURE.md   # Full technical documentation
```

## Architecture

### Transaction Flow (Publish / Purchase / Distribute)

Two-step pattern used throughout:
1. **Prepare** (Rust): Validate inputs, store pending row in DB (active=0 for content), build EVM calldata, return `TransactionRequest { to, data, value, description }`
2. **Sign** (Frontend): Send tx via Web3Modal/ethers to MetaMask, wait for on-chain confirmation
3. **Confirm** (Rust): Fetch tx receipt, extract event data (e.g. contentId from `ContentPublished`), mark DB row active, start seeding/gossip

### Key Patterns

- **`AppState`**: Shared via `tauri::State<AppState>`. iroh node is lazy-initialized via `ensure_iroh()`. Chain client created on-demand via `chain_client()` (HTTP providers are stateless/cheap). Wallet address in `Arc<Mutex<Option<String>>>`.
- **iroh `!Sync` workaround**: `DiscoveryService` and `GossipReceiver` are `!Sync`. The gossip actor confines them to a spawned task, exposing only an `mpsc::Sender<GossipCmd>` (Send+Sync) via `state.send_gossip()`.
- **Content IDs**: On-chain `contentId = keccak256(contentHash, creator, nonce)` — includes per-creator nonce so the same file can be published multiple times as separate listings. The contentId is extracted from the `ContentPublished` event in the tx receipt, not computed locally. BLAKE3 hash (iroh) and keccak256 content ID serve different purposes — never confuse them.
- **Tauri IPC**: Rust uses snake_case params; Tauri v2 auto-converts to camelCase on the frontend. Return `Result<T, String>` from all commands.
- **`TransactionRequest`**: `{ to: String, data: String, value: String, description: String }` — passed to frontend for MetaMask signing. Raw EIP-1193 calls (`eth_sendTransaction`) are used instead of ethers.js BrowserProvider to avoid WalletConnect compatibility issues.

### Smart Contracts (Sepolia)

| Contract | Address | Purpose |
|----------|---------|---------|
| MockARAToken | `0xA4c42cd49774d9B0af9C2D6BB88cf53b49b95b1b` | ERC-20 test token (mintable, 18 decimals) |
| AraStaking (proxy) | `0x16e1CA6619FF0555BAFc43dEC9595C39776A2B63` | Stake ARA to publish (10 ARA min) or seed (1 ARA/content) |
| AraContent (proxy) | `0x8C52B0b11cF5759312555ab1C6926e6Ce57297a0` | ERC-1155 content token (editions, nonce-based IDs, MIN_PRICE=0) |
| Marketplace (proxy) | `0xa133F5eb0aE369D627B13F0e283ACDC763Fb48c4` | ETH purchases + tipping, 85% creator / 2.5% stakers / 12.5% seeders |
| AraCollections (proxy) | `0x606658d5935E788CccCDF9188308434130a7C671` | On-chain collections for content grouping |
| AraNameRegistry (proxy) | `0x5C451d9B613468D4212AE31b5F139E759dD992FA` | Display name registry for wallet addresses |

### Reward System (Three-Way Split)

Purchase split: `85% creator / 2.5% stakers / 12.5% seeders`. Resale split: `royalty to creator / 1% stakers / 4% seeders / remainder to seller`. Tipping split: same as purchase (85/2.5/12.5), applied to the tip amount. Tips don't mint editions.

**Free content**: `MIN_PRICE = 0` allows publishing at price 0. Free content requires no on-chain purchase transaction to download (pure P2P). Staking requirement (10 ARA) still applies to publish free content.

**Passive staker rewards** (AraStaking V2): Uses a Synthetix-style reward accumulator for O(1) gas-efficient proportional distribution. On each purchase, Marketplace calls `staking.addReward{value: stakerReward}()` which updates `rewardPerTokenStored`. Each staker's payout is proportional to their staked ARA: `earned = userStake * (rewardPerTokenStored - userCheckpoint) / 1e18`. Claim via `claimStakingReward()`. Edge case: if `totalStaked == 0`, the staker share falls back to the seeder pool.

**Seeder rewards** (12.5% of purchases): Held in `buyerReward[contentId][buyer]` per-receipt. Seeders collect via `claimDeliveryReward()` or batch `claimDeliveryRewards()` with buyer-signed EIP-712 receipts.

**Anti-fraud**: Buyers sign `EIP-712 DeliveryReceipt(bytes32 contentId, address seederEthAddress, uint256 timestamp)` with their Ethereum wallet. Domain: `{ name: "AraMarketplace", version: "1", chainId: 11155111, verifyingContract: marketplace_addr }`. These receipts are broadcast on gossip and stored in `delivery_receipts` DB table.

### Gossip Protocol

Each content hash has a dedicated iroh-gossip topic. Messages (all `serde_json` serialized):
- `SeederAnnounce { content_hash, node_id_bytes }` — seeder joined
- `SeederLeave { content_hash, node_id_bytes }` — seeder left
- `DeliveryReceipt { content_id, seeder_eth_address, buyer_eth_address, signature: Vec<u8>, timestamp }` — buyer receipt
- `SeederIdentity { node_id, eth_address, signature: Vec<u8> }` — NodeId→ETH address link

The `GossipActor` runs in a background `tokio::spawn` task with an `mpsc::Sender<GossipCmd>` for commands and an unbounded channel for internal events. Handles NeighborUp re-announce heartbeat (every 60s).

### SQLite Schema

```sql
content (content_id PK, content_hash, creator, metadata_uri, price_wei, title, description,
         content_type, thumbnail_url, file_size_bytes, active, created_at,
         publisher_node_id, publisher_relay_url, filename)

purchases (content_id, buyer PK, price_paid_wei, tx_hash, purchased_at, downloaded_path)

seeding (content_id PK, active, bytes_served, peer_count, started_at)

rewards (id AUTOINCREMENT, content_id, amount_wei, tx_hash, claimed, distributed_at)

config (key PK, value)

content_seeders (content_hash, node_id PK, eth_address, discovered_at)

delivery_receipts (content_id, seeder_eth_address, buyer_eth_address PK, signature, timestamp)
```

## Build & Run

```bash
# Contracts
cd contracts && forge build && forge test -vvv

# Rust workspace
cargo check --workspace
cargo test --workspace

# Desktop app (dev mode)
pnpm install
pnpm dev            # or: cd app && pnpm tauri dev

# Deploy contracts (requires DEPLOYER_PRIVATE_KEY + SEPOLIA_RPC_URL env vars)
cd contracts && forge script script/Deploy.s.sol --rpc-url $SEPOLIA_RPC_URL --broadcast --verify
```

## Windows Notes

- cargo/node/pnpm not in shell PATH in some terminal sessions. Use full paths or set PATH:
  ```bash
  export PATH="/c/Program Files/nodejs:/c/Users/tmuga/AppData/Roaming/npm:/c/Users/tmuga/.cargo/bin:$PATH"
  ```
- Forge is at `C:/Users/tmuga/Code/libraries/foundry/forge.exe`
- Tauri needs `icons/icon.ico` in `app/src-tauri/icons/` for Windows builds
- Recompiled test binaries may be blocked by Windows App Control — `cargo clean -p <crate>` then rebuild

## Known Issues / Gotchas

- **alloy + serde compatibility**: Must use alloy 1.x (1.7.3+). serde >= 1.0.220 broke `serde::__private` used by older alloy.
- **serde array bounds**: `[u8; N]` only implements serde for N ≤ 32. Use `Vec<u8>` for signatures (65 bytes) and other large fixed arrays in `GossipMessage` enum variants.
- **iroh shutdown race**: Always `drop(content_mgr)` before `node.shutdown()`. Need `tokio::time::sleep(100ms)` after `delete_blob` before shutdown.
- **`test_two_node_transfer`**: Flaky integration test (iroh relay timing). Passes locally most of the time.
- **Web3Modal `open` conflict**: `useWeb3Modal().open` must be renamed (e.g. `openModal`) to avoid shadowing `@tauri-apps/plugin-dialog`'s `open` for the file picker.
- **iroh NodeAddr relay URL**: When building `NodeAddr` for cross-NAT downloads, always include the relay URL (stored as `publisher_relay_url` in the DB). A bare `NodeAddr::from(node_id)` without a relay URL will fail to connect across NATs.
- **WalletConnect + ethers.js**: `BrowserProvider` internally calls `eth_blockNumber` which WalletConnect doesn't proxy. Use raw `walletProvider.request({ method: "eth_sendTransaction" })` for transactions. For signatures (`eth_signTypedData_v4`), direct provider.request() also works.
- **Content ID vs Content Hash**: `content_id` is keccak256(blake3_hash, creator, nonce) — on-chain identifier. `content_hash` is the BLAKE3 hash — iroh P2P identifier and gossip topic key. Never mix these up.
- **Delivery receipts in gossip**: Receipts are broadcast on the gossip topic keyed by BLAKE3 `content_hash`, but contain the keccak256 `content_id` (for on-chain verification). Both identifiers are stored in the DB.

## Registered Tauri Commands

**Wallet**: `connect_wallet`, `disconnect_wallet`, `get_balances`

**Content**: `publish_content`, `confirm_publish`, `get_content_detail`, `search_content`, `update_content`, `confirm_update_content`, `get_my_content`, `get_published_content`, `delist_content`, `confirm_delist`, `update_content_file`, `confirm_content_file_update`, `import_preview_assets`, `get_preview_asset`

**Marketplace**: `purchase_content`, `confirm_purchase`, `tip_content`, `confirm_tip`, `get_library`, `open_downloaded_content`, `open_content_folder`, `broadcast_delivery_receipt`, `get_marketplace_address`, `get_receipt_count`, `list_for_resale`, `confirm_list_for_resale`, `cancel_resale_listing`, `confirm_cancel_listing`, `buy_resale`, `get_resale_listings`, `get_edition_info`

**Seeding**: `start_seeding`, `stop_seeding`, `get_seeder_stats`

**Staking**: `stake_ara`, `unstake_ara`, `stake_for_content`, `get_stake_info`, `claim_staking_reward`, `prepare_claim_rewards`, `confirm_claim_rewards`, `get_reward_history`, `get_reward_pipeline`

**Collections**: `create_collection`, `confirm_create_collection`, `update_collection`, `confirm_update_collection`, `delete_collection`, `confirm_delete_collection`, `add_to_collection`, `confirm_add_to_collection`, `remove_from_collection`, `confirm_remove_from_collection`, `get_my_collections`, `get_collection`, `get_collection_items`, `get_all_collections`, `get_content_collection`, `get_top_collections`

**Names**: `register_name`, `confirm_register_name`, `remove_display_name`, `confirm_remove_name`, `get_display_name`, `get_display_names`

**Analytics**: `get_price_history`, `get_item_analytics`, `get_top_collectors`, `get_trending_content`, `get_marketplace_overview`

**Utility**: `wait_for_transaction`, `sync_content`, `sync_rewards`
