import { createWeb3Modal, defaultConfig } from "@web3modal/ethers/react";

// WalletConnect project ID — get yours from https://cloud.walletconnect.com
const projectId =
  import.meta.env.VITE_WALLETCONNECT_PROJECT_ID || "PLACEHOLDER_PROJECT_ID";

// Chain ID where contracts are deployed — matches Rust backend config.
// Set via VITE_CHAIN_ID env var. Default: 11155111 (Sepolia testnet).
// Change to 1 when deploying to mainnet.
const activeChainId = Number(import.meta.env.VITE_CHAIN_ID) || 11155111;

const allChains = [
  {
    chainId: 1,
    name: "Ethereum",
    currency: "ETH",
    explorerUrl: "https://etherscan.io",
    rpcUrl: "https://eth.llamarpc.com",
  },
  {
    chainId: 11155111,
    name: "Sepolia",
    currency: "ETH",
    explorerUrl: "https://sepolia.etherscan.io",
    rpcUrl: "https://ethereum-sepolia.publicnode.com",
  },
];

const activeChain = allChains.find((c) => c.chainId === activeChainId) ?? allChains[0];

// Expose ONLY the active chain to Web3Modal. If we include extra chains
// (e.g., mainnet alongside Sepolia), WalletConnect can establish the session
// on the wrong chain and then refuse to switch because the session was pinned
// to the wrong chain at connect time. Restricting to one chain forces wallets
// to connect on the right network up front.
const chains = [activeChain];

// One-time migration: clear stale Web3Modal state from v1.0.5/1.0.6 (which passed a
// top-level `defaultChain` option). Without the sweep those caches produce
// "Cannot set properties of undefined (setting 'defaultChain')" on startup. Gated
// behind a version marker so subsequent sessions don't keep nuking user state —
// e.g., signed-in wallet metadata that the current SDK version writes and reads.
const MIGRATION_KEY = "ara.w3m_migrated_v1_0_9";
if (typeof window !== "undefined" && !localStorage.getItem(MIGRATION_KEY)) {
  try {
    const stale = Object.keys(localStorage).filter(
      (k) => k.startsWith("@w3m") || k.startsWith("wagmi.") || k === "W3M_RECENT_WALLET_DATA",
    );
    for (const k of stale) localStorage.removeItem(k);
    localStorage.setItem(MIGRATION_KEY, "1");
  } catch { /* ignore — localStorage may be disabled */ }
}

const metadata = {
  name: "Ara Marketplace",
  description: "Decentralized content marketplace — Stake ARA, Seed Content, Earn ETH",
  url: "https://ara.one",
  icons: [],
};

createWeb3Modal({
  // Pass defaultChainId via defaultConfig (the documented v3 API surface);
  // this drives the Coinbase-wallet provider's initial chain selection.
  ethersConfig: defaultConfig({
    metadata,
    defaultChainId: activeChainId,
    rpcUrl: activeChain.rpcUrl,
  }),
  chains,
  projectId,
  enableAnalytics: false,
});
