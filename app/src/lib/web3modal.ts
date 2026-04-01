import { createWeb3Modal, defaultConfig } from "@web3modal/ethers/react";

// WalletConnect project ID — get yours from https://cloud.walletconnect.com
// Set via VITE_WALLETCONNECT_PROJECT_ID env var, or use placeholder for dev
const projectId =
  import.meta.env.VITE_WALLETCONNECT_PROJECT_ID || "PLACEHOLDER_PROJECT_ID";

const mainnet = {
  chainId: 1,
  name: "Ethereum",
  currency: "ETH",
  explorerUrl: "https://etherscan.io",
  rpcUrl: "https://eth.llamarpc.com",
};

const sepolia = {
  chainId: 11155111,
  name: "Sepolia",
  currency: "ETH",
  explorerUrl: "https://sepolia.etherscan.io",
  rpcUrl: "https://ethereum-sepolia.publicnode.com",
};

const metadata = {
  name: "Ara Marketplace",
  description: "Decentralized content marketplace — Stake ARA, Seed Content, Earn ETH",
  url: "https://ara.one",
  icons: [],
};

createWeb3Modal({
  ethersConfig: defaultConfig({ metadata }),
  chains: [sepolia, mainnet],
  projectId,
  defaultChain: sepolia,
  enableAnalytics: false,
});
