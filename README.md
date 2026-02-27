# Ara Marketplace

**Own your content. Keep your revenue. Trust the math.**

Ara is a decentralized content marketplace where creators publish anything — music, video, software, documents — and keep 85% of every sale. The remaining 15% is split between the people who secure the network (ARA stakers, 2.5%) and the people who distribute the content (seeders, 12.5%). No platform cut. No gatekeepers. No single point of failure.

---

## Why Ara Exists

Digital marketplaces extract enormous value from creators and buyers while contributing almost nothing to the actual work of creation or distribution. Platforms take 30–50% cuts, arbitrarily delist content, freeze accounts without recourse, and can disappear overnight taking years of creator revenue with them.

Ara flips this. Every rule is enforced by open-source smart contracts on Ethereum. Every file is stored and transferred peer-to-peer. No company controls whether your content stays available or whether your payment goes through. If the Ara team disappeared tomorrow, the marketplace would keep running.

---

## How It Works

### For Creators
1. **Stake 10 ARA tokens** — a small deposit that signals serious participation
2. **Publish any file** — it gets hashed, stored in your iroh node, and registered on Ethereum
3. **Set your price in ETH** — you receive 85% of every purchase instantly, on-chain
4. **Distribute seeder rewards** — use the Library tab to allocate the 12.5% reward pool to the people who helped distribute your content

### For Buyers
1. **Browse the marketplace** — search by title, type, or creator
2. **Purchase with MetaMask** — ETH goes directly to the creator (no intermediary holds funds)
3. **Download via P2P** — content transfers encrypted, directly from seeders using iroh
4. **Seed and earn** — toggle seeding on any purchased content to share it and collect rewards

### For Stakers
1. **Stake any amount of ARA** — the more you stake, the larger your share of staking rewards
2. **Earn passively** — 2.5% of every primary purchase and 1% of every resale is distributed proportionally to all ARA stakers
3. **Claim anytime** — staking rewards accrue automatically on-chain; claim your ETH from the Wallet page whenever you want
4. **Secure the network** — staking provides economic security (sybil resistance), ensures serious participation, and lays the foundation for future governance

### For Seeders
1. **Stake 1 ARA for the content you seed** — signals commitment, makes you eligible for rewards
2. **Keep seeding running** — the longer and more reliably you seed, the more delivery receipts you accumulate
3. **Collect your share** — when the creator distributes rewards (or after 30 days via the trustless fallback), claim your ETH from the 12.5% seeder pool

---

## The Self-Reinforcing Flywheel

Popular content attracts more seeders. More seeders mean faster downloads and better availability. Better availability drives more purchases. More purchases grow both the seeder reward pool and staker rewards. A larger reward pool attracts more seeders. Higher staking rewards incentivize more ARA to be staked, strengthening network security. **The network gets stronger as it grows.**

---

## Reward Split

Every ETH payment on Ara is deterministically split by smart contracts — no platform fees, no hidden cuts.

### Primary Purchases
| Recipient | Share | How It Works |
|-----------|-------|-------------|
| Creator | 85% | Sent instantly on purchase |
| ARA Stakers | 2.5% | Distributed proportionally to all stakers by amount staked |
| Seeders | 12.5% | Distributed to seeders who deliver content, weighted by delivery receipts |

### Resale Purchases
| Recipient | Share | How It Works |
|-----------|-------|-------------|
| Seller | Remainder after fees | Sent instantly on resale |
| Creator | Royalty (set at publish) | Enforced on-chain, cannot be bypassed |
| ARA Stakers | 1% | Same proportional distribution as primary |
| Seeders | 4% | Same delivery-receipt mechanism as primary |

**Staking rewards are proportional**: if you stake 1% of the total ARA staked across all users, you earn 1% of the staker reward from every purchase. There is no minimum stake to earn — any staked ARA earns its proportional share.

---

## Anti-Fraud by Design

Seeder rewards are based on **cryptographic delivery receipts** — buyers sign a gasless EIP-712 message with their Ethereum wallet after each download, attesting that a specific seeder served them. These receipts are:

- **Unforgeable** — only the buyer's private key can create them, and on-chain `ecrecover` verifies this
- **Replay-protected** — each receipt can only be used once; the contract tracks which have been consumed
- **Trustless to verify** — the `publicDistributeWithProofs()` function verifies everything on-chain; no one needs to be trusted

A creator cannot fake receipts to pocket the reward pool (they don't hold any buyer private keys). A seeder cannot forge receipts (same reason). A bad actor cannot replay valid receipts (the contract marks them used).

---

## The Trustless Fallback

What if a creator goes dark and never distributes rewards? After **30 days** from the last purchase, any eligible seeder can call `publicDistributeWithProofs()` directly — submitting the buyer-signed receipts they've collected. The smart contract verifies every signature on-chain and distributes the pool proportionally. No trust required, no creator cooperation needed.

Millions of ETH in reward pools cannot be locked forever.

---

## Tech Stack

| Layer | Technology | Why |
|-------|-----------|-----|
| Desktop App | Tauri v2 (Rust + React/TypeScript) | Native performance, no Electron overhead |
| Smart Contracts | Solidity 0.8.24 on Ethereum (Sepolia) | Trustless payments and registry |
| P2P Transfer | iroh (Rust) | Encrypted, content-addressed, NAT-traversing |
| P2P Discovery | iroh-gossip | Permissionless seeder discovery per content |
| Wallet | MetaMask via WalletConnect / Web3Modal | Industry-standard wallet support |
| Local Storage | SQLite (rusqlite) | Fast, reliable, zero-config local state |
| Ethereum SDK | alloy 1.x (Rust) + ethers v6 (TypeScript) | Modern, strongly typed chain interaction |

---

## Quick Start

### Prerequisites

- **Rust** (stable, 1.75+) — `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **Node.js** (20+) + **pnpm** — `npm install -g pnpm`
- **Foundry** (for contract work) — `curl -L https://foundry.paradigm.xyz | bash`

### Setup Scripts (installs all prerequisites)

**Ubuntu/Debian:**
```bash
git clone https://github.com/AraBlocks/ara-marketplace.git
cd ara-marketplace
bash scripts/setup-ubuntu.sh
```

**macOS:**
```bash
git clone https://github.com/AraBlocks/ara-marketplace.git
cd ara-marketplace
bash scripts/setup-macos.sh
```

**Windows** (Administrator PowerShell):
```powershell
git clone https://github.com/AraBlocks/ara-marketplace.git
cd ara-marketplace
powershell -ExecutionPolicy Bypass -File scripts\setup-windows.ps1
```

### Run the App

```bash
# Add your WalletConnect project ID to app/.env
# Get one free at https://cloud.walletconnect.com
echo "VITE_WALLETCONNECT_PROJECT_ID=your_id_here" > app/.env

pnpm dev
```

The app opens as a native desktop window. Connect MetaMask on Sepolia testnet, get test ARA from the faucet, and start publishing or buying content.

### Contract Development

```bash
cd contracts
forge build          # Compile
forge test -vvv      # Run tests (73+ tests)

# Deploy to Sepolia (requires DEPLOYER_PRIVATE_KEY + SEPOLIA_RPC_URL)
forge script script/Deploy.s.sol --rpc-url $SEPOLIA_RPC_URL --broadcast --verify
```

---

## Project Structure

```
ara-marketplace/
├── contracts/              # Solidity smart contracts (Foundry project)
│   ├── src/                # AraStaking, AraContent, Marketplace, MockARAToken
│   ├── test/               # Forge tests (73+ tests)
│   └── script/             # Deploy.s.sol
├── crates/
│   ├── ara-core/           # Config, SQLite storage, shared types
│   ├── ara-p2p/            # iroh node, blob management, gossip discovery, seeding
│   └── ara-chain/          # Typed Ethereum clients (alloy-based)
├── app/
│   ├── src-tauri/src/      # Rust backend (Tauri commands, gossip actor, state)
│   │   └── commands/       # content, marketplace, seeding, staking, tx, wallet, sync
│   └── src/                # React frontend
│       ├── pages/          # Marketplace, Publish, ContentDetail, Library, Dashboard, Wallet
│       ├── lib/            # tauri.ts (IPC bindings), transactions.ts, web3modal.ts
│       └── store/          # walletStore.ts (Zustand)
└── docs/
    └── ARCHITECTURE.md     # Full technical documentation
```

---

## Live Contracts (Sepolia Testnet)

| Contract | Address |
|----------|---------|
| MockARAToken | `0x53720EcdDF71fE618c7A5aEc99ac2e958ad4dF99` |
| AraStaking (proxy) | `0xfD41Ae37cD729b6a70e42641ea14187e213b29e6` |
| AraContent (proxy) | `0xd45ff950bBC1c823F66C4EbdF72De23Eb02e4831` |
| Marketplace (proxy) | `0xD7992b6A863FBacE3BB58BFE5D31EAe580adF4E0` |
| AraCollections (proxy) | `0x59453f1f12D10e4B4210fae8188d666011292997` |
| AraNameRegistry (proxy) | `0xDA5827A8659271C44174894bbA403FD264198C5d` |

All contracts are verified on [Sepolia Etherscan](https://sepolia.etherscan.io).

---

## Current Status

Ara is in active development on Sepolia testnet. Core flows are fully functional:

- [x] Content publishing (stake → hash → register on-chain → announce via gossip)
- [x] Content purchasing (ETH payment → P2P download → auto-seeding)
- [x] Seeder discovery and transfer via iroh
- [x] Staking and eligibility checks
- [x] Delivery receipt signing and gossip broadcast
- [x] Creator reward distribution (fast path)
- [x] Trustless fallback distribution (after 30-day window)
- [x] Reward claiming
- [x] Passive staking rewards (2.5% of purchases, proportional to stake)
- [x] Resale marketplace with royalties and staker/seeder reward split

In progress:
- [x] ERC-1155 content tokens with edition support (maxSupply, minting on purchase)
- [ ] Seeder identity broadcast on startup (NodeId → ETH address linking)
- [ ] Global content discovery feed

---

## Documentation

- [Architecture & Technical Reference](docs/ARCHITECTURE.md) — complete system design, all flows, all components

---

## License

LGPL-3.0
