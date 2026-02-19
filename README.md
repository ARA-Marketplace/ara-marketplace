# Ara Marketplace

**A decentralized content marketplace where creators publish, buyers pay with ETH, and seeders earn by sharing.**

## The Problem

Digital content distribution is broken. Platforms take massive cuts (30-50%), creators have no control over pricing, and centralized servers are single points of failure. When a platform goes down or changes its rules, creators and buyers both lose.

## How Ara Fixes This

Ara is a peer-to-peer content marketplace that cuts out the middleman:

- **Creators** publish any file (music, video, documents, software) and set their own price in ETH
- **Buyers** purchase content directly — 85% goes straight to the creator
- **Seeders** share purchased content with the network and earn ETH rewards from the remaining 15%
- **No central server** — content is distributed across the network using peer-to-peer technology

The more popular content gets, the more seeders share it, the faster it downloads, and the more everyone earns. It's a self-reinforcing flywheel.

## How It Works

1. **Stake ARA tokens** to participate (10 ARA to publish, 1 ARA to seed)
2. **Publish** a file — it gets hashed, stored locally, and registered on the Ethereum blockchain
3. **Browse & buy** — search the marketplace, purchase with ETH via MetaMask
4. **Seed & earn** — toggle seeding on purchased content to share it and earn rewards

Every transaction is recorded on Ethereum. Content is transferred peer-to-peer using [iroh](https://iroh.computer/), a fast, encrypted networking protocol. No content ever touches a central server.

## Tech Stack

| Layer | Technology | Purpose |
|-------|-----------|---------|
| Desktop App | Tauri v2 (Rust + React) | Native app with web UI |
| Smart Contracts | Solidity on Ethereum | Payments, staking, content registry |
| P2P Network | iroh | Encrypted content transfer between peers |
| Wallet | MetaMask via WalletConnect | Transaction signing |

## Quick Start

**Prerequisites:** Rust, Node.js, pnpm, [Foundry](https://getfoundry.sh/) (for contracts)

```bash
# Install dependencies
pnpm install

# Run in development mode
pnpm dev
```

The app will open as a native desktop window. Connect your MetaMask wallet (Sepolia testnet), stake some test ARA tokens, and start publishing or purchasing content.

## Project Structure

```
contracts/     Smart contracts (Foundry) — staking, registry, marketplace
crates/        Rust libraries — P2P networking, Ethereum integration, storage
app/           Tauri desktop app — React frontend + Rust backend
```

## Current Status

Ara is in active development on the Sepolia testnet. The core publish, purchase, and seeding flows are fully functional. Cross-node content discovery and download are coming next.

## License

LGPL-3.0
