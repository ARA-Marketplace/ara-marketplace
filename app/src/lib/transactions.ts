import type { Eip1193Provider } from "ethers";
import { invoke } from "@tauri-apps/api/core";

/** Mirrors the Rust TransactionRequest struct returned by IPC commands. */
export interface TransactionRequest {
  to: string;
  data: string;
  value: string;
  description: string;
}

const EXPECTED_CHAIN_ID = Number(import.meta.env.VITE_CHAIN_ID) || 11155111;

function chainName(id: number): string {
  if (id === 1) return "Ethereum mainnet";
  if (id === 11155111) return "Sepolia testnet";
  return `chain ${id}`;
}

function expectedChainHex(): string {
  // MetaMask expects minimal-byte hex (no leading zeros) — '0x' + hex w/o padding
  return "0x" + EXPECTED_CHAIN_ID.toString(16);
}

/**
 * Decide whether a raw chainId reported by the wallet matches our expected chain.
 *
 * EIP-1193 specifies `eth_chainId` returns a hex string, but in practice wallets
 * (especially MetaMask mobile over WalletConnect) sometimes return a number, a
 * bigint, or null. Normalize first, then compare.
 *
 * MetaMask mobile's "multichain view" also introduced an encoding bug where it
 * returns e.g. "0x11155111" — the decimal digits of Sepolia's chainId treated as
 * hex — instead of the real "0xaa36a7". Both forms clearly refer to the same
 * chain, so accept either.
 */
function normalizeChainId(raw: unknown): string | null {
  if (typeof raw === "string") return raw.toLowerCase();
  if (typeof raw === "number" && Number.isFinite(raw)) return "0x" + raw.toString(16);
  if (typeof raw === "bigint") return "0x" + raw.toString(16);
  return null;
}

function chainIdMatches(raw: unknown, expected: number): boolean {
  const normalized = normalizeChainId(raw);
  if (!normalized) return false;
  const realHex = "0x" + expected.toString(16);
  const decimalAsHex = "0x" + String(expected); // the buggy-encoding form MetaMask sometimes returns
  return normalized === realHex || normalized === decimalAsHex;
}

function parseChainIdToNumber(raw: unknown): number {
  if (typeof raw === "number" && Number.isFinite(raw)) return raw;
  if (typeof raw === "bigint") return Number(raw);
  if (typeof raw === "string") return parseInt(raw, 16);
  return NaN;
}

/**
 * Sign and send a list of transactions sequentially using the connected wallet.
 *
 * Uses walletProvider.request({ method: "eth_sendTransaction" }) directly
 * instead of ethers.js BrowserProvider, because ethers v6 internally calls
 * eth_blockNumber for gas estimation — a method WalletConnect does not proxy.
 * MetaMask/WalletConnect handle nonce and gas estimation on their side.
 *
 * Receipt confirmation is delegated to the Rust backend which has a proper
 * HTTP provider for eth_getTransactionReceipt polling.
 *
 * Calls onStatus(message) after each milestone so the UI can update.
 * Returns the tx hash of the last transaction.
 */
export async function signAndSendTransactions(
  walletProvider: Eip1193Provider,
  transactions: TransactionRequest[],
  onStatus?: (msg: string) => void
): Promise<string> {
  // Check current chain and auto-switch if needed.
  //
  // @web3modal/ethers 3.5.x has a bug in its WalletConnect UniversalProvider
  // chainChanged handler: after a successful `wallet_switchEthereumChain` it
  // throws "Cannot set properties of undefined (setting 'defaultChain')" in its
  // internal namespace bookkeeping. The actual chain switch on the wallet side
  // still succeeds — only Web3Modal's state tracking breaks.
  //
  // Strategy: request the switch, swallow any error Web3Modal throws, then
  // RE-QUERY `eth_chainId` ourselves. If the wallet actually switched, proceed.
  // If not, throw a clear error with wallet-specific recovery instructions.
  let chainId: unknown = await walletProvider.request({ method: "eth_chainId" });
  let currentChainId = parseChainIdToNumber(chainId);
  console.debug(`[Ara] Wallet chain check: raw=${String(chainId)} (type=${typeof chainId}) parsed=${currentChainId} expected=${EXPECTED_CHAIN_ID}`);

  if (!chainIdMatches(chainId, EXPECTED_CHAIN_ID)) {
    onStatus?.(`Switching wallet to ${chainName(EXPECTED_CHAIN_ID)}…`);
    const targetHex = expectedChainHex();
    try {
      await walletProvider.request({
        method: "wallet_switchEthereumChain",
        params: [{ chainId: targetHex }],
      });
    } catch (e: unknown) {
      const err = e as { code?: number; message?: string };
      // 4902 = chain not added to wallet — try to add it
      if (err?.code === 4902 || err?.message?.includes("Unrecognized chain")) {
        try {
          await walletProvider.request({
            method: "wallet_addEthereumChain",
            params: [
              EXPECTED_CHAIN_ID === 11155111
                ? {
                    chainId: targetHex,
                    chainName: "Sepolia",
                    nativeCurrency: { name: "Sepolia ETH", symbol: "ETH", decimals: 18 },
                    rpcUrls: ["https://ethereum-sepolia.publicnode.com"],
                    blockExplorerUrls: ["https://sepolia.etherscan.io"],
                  }
                : {
                    chainId: targetHex,
                    chainName: "Ethereum",
                    nativeCurrency: { name: "Ether", symbol: "ETH", decimals: 18 },
                    rpcUrls: ["https://eth.llamarpc.com"],
                    blockExplorerUrls: ["https://etherscan.io"],
                  },
            ],
          });
        } catch (addErr) {
          console.warn("[Ara] wallet_addEthereumChain failed:", addErr);
        }
      } else {
        // Could be the Web3Modal defaultChain bug — the underlying wallet may
        // still have switched. Log and continue to the re-check below.
        console.warn("[Ara] wallet_switchEthereumChain threw (may be benign):", err);
      }
    }

    // Re-check. If the wallet actually switched despite the Web3Modal error,
    // we're good. Otherwise show the detailed recovery instructions.
    chainId = await walletProvider.request({ method: "eth_chainId" });
    currentChainId = parseChainIdToNumber(chainId);
    console.debug(`[Ara] Post-switch chain: raw=${String(chainId)} (type=${typeof chainId}) parsed=${currentChainId}`);

    if (!chainIdMatches(chainId, EXPECTED_CHAIN_ID)) {
      throw new Error(
        `Your wallet is on ${chainName(currentChainId)}, but Ara Marketplace needs ${chainName(EXPECTED_CHAIN_ID)}.\n\n` +
          `Recent MetaMask versions introduced a multi-chain view that doesn't always honor switch requests from dapps. To fix:\n\n` +
          `1. Disconnect your wallet from Ara (click your address in the bottom-left corner → Disconnect)\n` +
          `2. In MetaMask mobile: tap the account/network selector at the top → Networks → switch to ${chainName(EXPECTED_CHAIN_ID)} (add it if missing)\n` +
          `3. Reconnect to Ara by scanning the WalletConnect QR code again\n\n` +
          `On desktop MetaMask: use the extension's network dropdown before re-opening Ara.`,
      );
    }
  }

  // Get the connected address without prompting (eth_accounts = already-connected)
  const accounts = (await walletProvider.request({
    method: "eth_accounts",
  })) as string[];
  const from = accounts[0];
  if (!from) throw new Error("No account connected in wallet");

  let lastHash = "";
  const total = transactions.length;

  for (let i = 0; i < transactions.length; i++) {
    const tx = transactions[i];
    onStatus?.(
      `Confirm transaction ${i + 1}/${total} in your wallet: ${tx.description}`
    );

    // Send directly via the EIP-1193 request API — no ethers.js pre-flight calls.
    // MetaMask/WalletConnect will handle gas estimation, nonce, and signing.
    const txHash = (await walletProvider.request({
      method: "eth_sendTransaction",
      params: [
        {
          from,
          to: tx.to,
          data: tx.data,
          value: tx.value,
          chainId: expectedChainHex(), // prevent submission on the wrong network
        },
      ],
    })) as string;

    lastHash = txHash;
    onStatus?.(
      `Transaction ${i + 1}/${total} submitted (${lastHash.slice(0, 10)}…). Waiting for confirmation…`
    );

    // Use the Rust backend's HTTP provider to poll for the receipt.
    await invoke("wait_for_transaction", { txHash: lastHash });
    onStatus?.(`Transaction ${i + 1}/${total} confirmed.`);
  }

  return lastHash;
}
