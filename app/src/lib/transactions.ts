import type { Eip1193Provider } from "ethers";
import { invoke } from "@tauri-apps/api/core";

/** Mirrors the Rust TransactionRequest struct returned by IPC commands. */
export interface TransactionRequest {
  to: string;
  data: string;
  value: string;
  description: string;
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
  // Ensure wallet is on Sepolia (chainId 11155111 = 0xaa36a7).
  // MetaMask tends to revert to mainnet, so we switch automatically.
  const chainId = (await walletProvider.request({ method: "eth_chainId" })) as string;
  if (parseInt(chainId, 16) !== 11155111) {
    onStatus?.("Switching wallet to Sepolia testnet…");
    await walletProvider.request({
      method: "wallet_switchEthereumChain",
      params: [{ chainId: "0xaa36a7" }],
    });
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
          chainId: "0xaa36a7", // Sepolia — prevents mainnet submission
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
