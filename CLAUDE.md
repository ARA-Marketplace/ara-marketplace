# Ara Marketplace - Technical Reference

## What This Is

A decentralized content marketplace desktop app. Creators publish files, buyers purchase with ETH, seeders distribute content peer-to-peer and earn rewards. Think "BitTorrent meets an app store, with Ethereum payments."

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
    src/              # AraStaking, ContentRegistry, Marketplace, MockARAToken
    test/             # Forge tests (28+ tests)
    script/           # Deploy.s.sol
  crates/
    ara-core/         # Config, SQLite storage, shared types
    ara-p2p/          # iroh node, content management, discovery, seeding, metrics
    ara-chain/        # Typed Ethereum clients (token, staking, registry, marketplace, events)
  app/
    src-tauri/src/    # Tauri commands (Rust backend)
      commands/       # content, marketplace, seeding, staking, tx, wallet, types
      state.rs        # AppState (config, DB, iroh node, gossip sender)
      setup.rs        # App initialization
      gossip_actor.rs # Background gossip task (mpsc channel pattern)
    src/              # React frontend
      pages/          # Marketplace, Publish, ContentDetail, Library, Dashboard, Wallet
      components/     # Navbar, Layout
      lib/            # tauri.ts (IPC bindings), transactions.ts, web3modal.ts, types.ts
      store/          # walletStore.ts (Zustand)
```

## Architecture

### Transaction Flow (Publish / Purchase)

Two-step pattern:
1. **Prepare** (Rust): Validate inputs, store pending row in DB (active=0), build EVM calldata, return `TransactionRequest { to, data, value, description }`
2. **Sign** (Frontend): Send tx via Web3Modal/ethers to MetaMask, wait for on-chain confirmation
3. **Confirm** (Rust): Fetch tx receipt, extract event data (e.g. contentId from `ContentPublished`), mark DB row active, start seeding, announce on gossip

### Key Patterns

- **`AppState`**: Shared via `tauri::State<AppState>`. iroh node is lazy-initialized via `ensure_iroh()`. Chain client is created on-demand via `chain_client()` (HTTP providers are stateless/cheap).
- **iroh `!Sync` workaround**: `DiscoveryService` and `GossipReceiver` are `!Sync`. The gossip actor confines them to a spawned task, exposing only an `mpsc::Sender<GossipCmd>` (Send+Sync).
- **Content IDs**: On-chain `contentId = keccak256(contentHash, creator, nonce)` — includes per-creator nonce so the same file can be published multiple times as separate listings. The contentId is extracted from the `ContentPublished` event in the tx receipt, not computed locally.
- **Tauri IPC**: Rust uses snake_case params; Tauri v2 auto-converts to camelCase on the frontend.

### Smart Contracts (Sepolia)

| Contract | Address | Purpose |
|----------|---------|---------|
| MockARAToken | `0xE8486e01aA1Da716448a3893792837AF9f1bBFa2` | ERC-20 test token (mintable) |
| AraStaking | `0x119554583bDB704CdA18f674054C2C7EF4C2A60c` | Stake ARA to publish (10 min) or seed (1 min) |
| ContentRegistry | `0x2ECb7C21A99BcB52CD202a94484C935b31cB0Ea0` | Register content on-chain (nonce-based IDs) |
| Marketplace | `0xA4bBCCBFc6F7C12ad80c45C0aed386289636Bb6E` | ETH purchases, 85% to creator, 15% reward pool |

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
- **iroh shutdown race**: Always `drop(content_mgr)` before `node.shutdown()`. Need `tokio::time::sleep(100ms)` after `delete_blob` before shutdown.
- **`test_two_node_transfer`**: Flaky integration test (iroh relay timing). Passes locally most of the time.
- **Web3Modal `open` conflict**: `useWeb3Modal().open` must be renamed (e.g. `openModal`) to avoid shadowing `@tauri-apps/plugin-dialog`'s `open` for the file picker.
- **iroh NodeAddr relay URL**: When building `NodeAddr` for cross-NAT downloads, always include the relay URL (stored as `publisher_relay_url` in the DB). A bare `NodeAddr::from(node_id)` without a relay URL will fail to connect across NATs.
