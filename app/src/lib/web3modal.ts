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

// Put the active chain first — Web3Modal uses chains[0] as the default
// when no explicit defaultChain is set. Passing a top-level `defaultChain`
// triggers "Cannot set properties of undefined" in @web3modal/ethers 3.5.x.
const chains = [
  allChains.find((c) => c.chainId === activeChainId) ?? allChains[0],
  ...allChains.filter((c) => c.chainId !== activeChainId),
];

const metadata = {
  name: "Ara Marketplace",
  description: "Decentralized content marketplace — Stake ARA, Seed Content, Earn ETH",
  url: "https://ara.one",
  icons: [],
};

createWeb3Modal({
  ethersConfig: defaultConfig({ metadata }),
  chains,
  projectId,
  enableAnalytics: false,
});
