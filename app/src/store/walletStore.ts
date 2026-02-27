import { create } from "zustand";
import {
  connectWallet as connectWalletIpc,
  disconnectWallet as disconnectWalletIpc,
  getBalances,
  getStakeInfo,
  stakeAra as stakeAraIpc,
  unstakeAra as unstakeAraIpc,
  claimStakingReward as claimStakingRewardIpc,
  syncRewards as syncRewardsIpc,
} from "../lib/tauri";
import { signAndSendTransactions } from "../lib/transactions";
import type { Eip1193Provider } from "ethers";

interface WalletState {
  // Connection state
  address: string | null;
  isConnecting: boolean;

  // Balances (formatted decimal strings, e.g. "1.5")
  ethBalance: string;
  araBalance: string;
  araStaked: string;

  // Passive staker rewards
  stakerRewardEarned: string;

  // Loading / error state
  isLoadingBalances: boolean;
  isSendingTx: boolean;
  txStatus: string | null;
  error: string | null;

  // Actions
  onWalletConnected: (address: string) => Promise<void>;
  onWalletDisconnected: () => Promise<void>;
  refreshBalances: () => Promise<void>;
  stakeAra: (amount: string, walletProvider: Eip1193Provider) => Promise<string>;
  unstakeAra: (amount: string, walletProvider: Eip1193Provider) => Promise<string>;
  claimStakingReward: (walletProvider: Eip1193Provider) => Promise<string>;
  clearError: () => void;
  clearTxStatus: () => void;
}

export const useWalletStore = create<WalletState>((set, get) => ({
  address: null,
  isConnecting: false,
  ethBalance: "0",
  araBalance: "0",
  araStaked: "0",
  stakerRewardEarned: "0",
  isLoadingBalances: false,
  isSendingTx: false,
  txStatus: null,
  error: null,

  onWalletConnected: async (address: string) => {
    set({ isConnecting: true, error: null });
    try {
      await connectWalletIpc(address);
      set({ address, isConnecting: false });
      // Auto-refresh balances after connecting
      await get().refreshBalances();
      // Sync reward history from chain in background (rebuilds on fresh install)
      syncRewardsIpc().catch(() => {});
    } catch (e) {
      set({
        error: `Failed to connect: ${e}`,
        isConnecting: false,
      });
    }
  },

  onWalletDisconnected: async () => {
    try {
      await disconnectWalletIpc();
    } catch {
      // Ignore backend errors on disconnect
    }
    set({
      address: null,
      ethBalance: "0",
      araBalance: "0",
      araStaked: "0",
      stakerRewardEarned: "0",
      txStatus: null,
      error: null,
    });
  },

  refreshBalances: async () => {
    const { address } = get();
    if (!address) return;

    set({ isLoadingBalances: true });
    try {
      const balances = await getBalances();
      const stakeInfo = await getStakeInfo();

      set({
        ethBalance: balances.eth_balance,
        araBalance: balances.ara_balance,
        araStaked: stakeInfo.total_staked,
        stakerRewardEarned: stakeInfo.staker_reward_earned,
        isLoadingBalances: false,
      });
    } catch (e) {
      set({
        error: `Failed to refresh balances: ${e}`,
        isLoadingBalances: false,
      });
    }
  },

  stakeAra: async (amount: string, walletProvider: Eip1193Provider) => {
    set({ isSendingTx: true, txStatus: "Building transactions...", error: null });
    try {
      const txRequests = await stakeAraIpc(amount);
      const txHash = await signAndSendTransactions(
        walletProvider,
        txRequests,
        (msg) => set({ txStatus: msg })
      );
      set({ txStatus: `Staked successfully! Tx: ${txHash.slice(0, 10)}…`, isSendingTx: false });
      await get().refreshBalances();
      return txHash;
    } catch (e) {
      set({ error: `Stake failed: ${e}`, isSendingTx: false, txStatus: null });
      throw e;
    }
  },

  unstakeAra: async (amount: string, walletProvider: Eip1193Provider) => {
    set({ isSendingTx: true, txStatus: "Building transaction...", error: null });
    try {
      const txRequests = await unstakeAraIpc(amount);
      const txHash = await signAndSendTransactions(
        walletProvider,
        txRequests,
        (msg) => set({ txStatus: msg })
      );
      set({ txStatus: `Unstaked successfully! Tx: ${txHash.slice(0, 10)}…`, isSendingTx: false });
      await get().refreshBalances();
      return txHash;
    } catch (e) {
      set({ error: `Unstake failed: ${e}`, isSendingTx: false, txStatus: null });
      throw e;
    }
  },

  claimStakingReward: async (walletProvider: Eip1193Provider) => {
    set({ isSendingTx: true, txStatus: "Building claim transaction...", error: null });
    try {
      const txRequests = await claimStakingRewardIpc();
      const txHash = await signAndSendTransactions(
        walletProvider,
        txRequests,
        (msg) => set({ txStatus: msg })
      );
      set({ txStatus: `Claimed successfully! Tx: ${txHash.slice(0, 10)}…`, isSendingTx: false });
      await get().refreshBalances();
      return txHash;
    } catch (e) {
      set({ error: `Claim failed: ${e}`, isSendingTx: false, txStatus: null });
      throw e;
    }
  },

  clearError: () => set({ error: null }),
  clearTxStatus: () => set({ txStatus: null }),
}));
