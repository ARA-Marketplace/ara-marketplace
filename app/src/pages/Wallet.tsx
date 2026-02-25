import { useState, useEffect, useCallback } from "react";
import {
  useWeb3Modal,
  useWeb3ModalAccount,
  useWeb3ModalProvider,
} from "@web3modal/ethers/react";
import { useWalletStore } from "../store/walletStore";
import {
  getRewardHistory,
  syncRewards,
  type RewardHistoryItem,
  type RewardHistoryResponse,
} from "../lib/tauri";

const PAGE_SIZE = 10;

function fmtDate(ts: number) {
  // Block numbers are used as approx timestamps from sync;
  // real timestamps (from confirm commands) are unix seconds.
  if (ts > 1_000_000_000) {
    return new Date(ts * 1000).toLocaleDateString();
  }
  return `Block ${ts}`;
}

function Wallet() {
  const { open } = useWeb3Modal();
  const { isConnected } = useWeb3ModalAccount();
  const { walletProvider } = useWeb3ModalProvider();

  const {
    address, ethBalance, araBalance, araStaked, claimableRewards,
    isLoadingBalances, isSendingTx, txStatus, error,
    refreshBalances, stakeAra, unstakeAra, claimRewards, clearError, clearTxStatus,
  } = useWalletStore();

  const [showStakeModal, setShowStakeModal] = useState(false);
  const [stakeMode, setStakeMode] = useState<"stake" | "unstake">("stake");
  const [stakeAmount, setStakeAmount] = useState("");

  // Reward history state
  const [rewardHistory, setRewardHistory] = useState<RewardHistoryResponse | null>(null);
  const [historyItems, setHistoryItems] = useState<RewardHistoryItem[]>([]);
  const [historyOffset, setHistoryOffset] = useState(0);
  const [hasMore, setHasMore] = useState(false);
  const [loadingHistory, setLoadingHistory] = useState(false);

  const fetchRewardHistory = useCallback(async (offset: number, append: boolean) => {
    setLoadingHistory(true);
    try {
      const data = await getRewardHistory(PAGE_SIZE, offset);
      setRewardHistory(data);
      if (append) {
        setHistoryItems((prev) => [...prev, ...data.items]);
      } else {
        setHistoryItems(data.items);
      }
      setHasMore(data.items.length >= PAGE_SIZE);
    } catch {
      // Silently fail — history is supplementary
    } finally {
      setLoadingHistory(false);
    }
  }, []);

  useEffect(() => {
    if (address) {
      setHistoryOffset(0);
      fetchRewardHistory(0, false);
    } else {
      setRewardHistory(null);
      setHistoryItems([]);
    }
  }, [address, fetchRewardHistory]);

  // Refresh balances and history on every mount (handles navigation from other pages).
  // Sync rewards from chain first to ensure DB has the latest distribute/claim events.
  useEffect(() => {
    if (address) {
      refreshBalances();
      setHistoryOffset(0);
      // Sync from chain, then fetch history (so newly distributed/claimed rewards appear)
      syncRewards()
        .catch(() => {})
        .finally(() => fetchRewardHistory(0, false));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const handleLoadMore = () => {
    const newOffset = historyOffset + PAGE_SIZE;
    setHistoryOffset(newOffset);
    fetchRewardHistory(newOffset, true);
  };

  const handleStake = async () => {
    if (!stakeAmount) return;
    if (!walletProvider) { open(); return; }
    try {
      if (stakeMode === "stake") await stakeAra(stakeAmount, walletProvider);
      else await unstakeAra(stakeAmount, walletProvider);
      setShowStakeModal(false);
      setStakeAmount("");
    } catch { /* error set in store */ }
  };

  const handleClaim = async () => {
    if (!walletProvider) { open(); return; }
    try {
      await claimRewards(walletProvider);
      // Refresh history after claiming (claimRewards already syncs internally)
      setHistoryOffset(0);
      await fetchRewardHistory(0, false);
    }
    catch { /* error set in store */ }
  };

  return (
    <div className="max-w-2xl">
      <div className="mb-6">
        <h1 className="page-title">Wallet</h1>
        <p className="page-subtitle">Manage your ARA tokens, ETH, and staking.</p>
      </div>

      {error && (
        <div className="alert-error mb-4 flex justify-between items-center">
          <span>{error}</span>
          <button onClick={clearError} className="text-xs font-medium ml-4 hover:opacity-70 flex-shrink-0">
            Dismiss
          </button>
        </div>
      )}
      {txStatus && (
        <div className="alert-info mb-4 flex justify-between items-center">
          <span>{txStatus}</span>
          {!isSendingTx && (
            <button onClick={clearTxStatus} className="text-xs font-medium ml-4 hover:opacity-70 flex-shrink-0">
              Dismiss
            </button>
          )}
        </div>
      )}

      {!address ? (
        <div className="card p-10 text-center">
          <p className="text-slate-500 dark:text-slate-400 mb-5">
            Connect your wallet to manage tokens and staking.
          </p>
          <button onClick={() => open()} className="btn-primary">
            {isConnected ? "Switch Wallet" : "Connect Wallet"}
          </button>
        </div>
      ) : (
        <div className="space-y-4">
          {/* Address card */}
          <div className="card p-5">
            <div className="flex justify-between items-center">
              <div>
                <p className="text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-500 mb-1">
                  Connected Address
                </p>
                <p className="font-mono text-sm text-slate-900 dark:text-slate-100 break-all">{address}</p>
              </div>
              <button
                onClick={refreshBalances}
                disabled={isLoadingBalances}
                className="btn-ghost text-xs flex-shrink-0 ml-4"
              >
                {isLoadingBalances ? "Refreshing…" : "Refresh"}
              </button>
            </div>
          </div>

          {/* Balances */}
          <div className="grid grid-cols-2 gap-4">
            {[
              { label: "ETH Balance",  value: `${ethBalance} ETH` },
              { label: "ARA Balance",  value: `${araBalance} ARA` },
            ].map(({ label, value }) => (
              <div key={label} className="card p-5">
                <p className="text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-500 mb-1">
                  {label}
                </p>
                <p className="text-xl font-bold text-slate-900 dark:text-slate-100">{value}</p>
              </div>
            ))}
          </div>

          {/* Staking */}
          <div className="card p-5">
            <p className="text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-500 mb-4">
              Staking
            </p>
            <div className="flex justify-between items-center">
              <div>
                <p className="text-2xl font-bold text-slate-900 dark:text-slate-100">{araStaked} <span className="text-sm font-normal text-slate-500">ARA</span></p>
                <p className="text-xs text-slate-500 dark:text-slate-500 mt-0.5">
                  Stake ARA to publish content and seed for rewards.
                </p>
              </div>
              <div className="flex gap-2 flex-shrink-0 ml-4">
                <button
                  onClick={() => { setStakeMode("stake"); setStakeAmount(""); setShowStakeModal(true); }}
                  disabled={isSendingTx}
                  className="btn-primary text-sm px-4 py-2"
                >
                  Stake
                </button>
                <button
                  onClick={() => { setStakeMode("unstake"); setStakeAmount(""); setShowStakeModal(true); }}
                  disabled={isSendingTx}
                  className="btn-secondary text-sm px-4 py-2"
                >
                  Unstake
                </button>
              </div>
            </div>
          </div>

          {/* Rewards Summary — 3 columns */}
          <div className="grid grid-cols-3 gap-4">
            <div className="card p-5">
              <p className="text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-500 mb-0.5">
                Lifetime Earnings
              </p>
              <p className="text-[10px] text-slate-400 dark:text-slate-600 mb-2">
                Total rewards distributed to you
              </p>
              <p className="text-xl font-bold text-slate-900 dark:text-slate-100">
                {rewardHistory?.total_earned_eth ?? claimableRewards} <span className="text-sm font-normal text-slate-500">ETH</span>
              </p>
            </div>
            <div className="card p-5">
              <p className="text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-500 mb-0.5">
                Ready to Claim
              </p>
              <p className="text-[10px] text-slate-400 dark:text-slate-600 mb-2">
                Withdraw to your wallet
              </p>
              <p className="text-xl font-bold text-emerald-600 dark:text-emerald-400">
                {rewardHistory?.claimable_eth ?? claimableRewards} <span className="text-sm font-normal text-slate-500">ETH</span>
              </p>
              <button
                onClick={handleClaim}
                disabled={isSendingTx || claimableRewards === "0" || claimableRewards === "0.0"}
                className="btn-success w-full mt-3 px-3 py-1.5 text-xs"
              >
                Claim Rewards
              </button>
            </div>
            <div className="card p-5">
              <p className="text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-500 mb-0.5">
                Withdrawn
              </p>
              <p className="text-[10px] text-slate-400 dark:text-slate-600 mb-2">
                Already sent to your wallet
              </p>
              <p className="text-xl font-bold text-slate-900 dark:text-slate-100">
                {rewardHistory?.total_claimed_eth ?? "0"} <span className="text-sm font-normal text-slate-500">ETH</span>
              </p>
            </div>
          </div>

          {/* How Rewards Work */}
          <div className="card p-5">
            <p className="text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-500 mb-3">
              How Rewards Work
            </p>
            <div className="text-xs text-slate-600 dark:text-slate-400 space-y-1.5">
              <p>When buyers purchase content, <span className="font-medium text-slate-700 dark:text-slate-300">15% goes to the reward pool</span>.</p>
              <p>Seeders who deliver content to buyers earn signed delivery receipts.</p>
              <p>Your reward share = <span className="font-medium text-slate-700 dark:text-slate-300">deliveries x ARA staked per content</span>.</p>
              <p>Stake more ARA per content to earn a larger share of the reward pool.</p>
            </div>
          </div>

          {/* Reward History */}
          <div className="card overflow-hidden">
            <div className="px-5 py-4 border-b border-slate-200 dark:border-slate-800">
              <p className="text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-500">
                Reward History
              </p>
            </div>
            {historyItems.length === 0 && !loadingHistory ? (
              <div className="px-5 py-8 text-center text-sm text-slate-400 dark:text-slate-600">
                No reward events yet. Distribute rewards from your Published content to see them here.
              </div>
            ) : (
              <table className="w-full text-sm">
                <thead className="border-b border-slate-200 dark:border-slate-800">
                  <tr>
                    {["Content", "Amount", "Status", "Date", "Tx"].map((h) => (
                      <th key={h} className="px-4 py-2.5 text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-500 text-left">
                        {h}
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody className="divide-y divide-slate-100 dark:divide-slate-800/60">
                  {historyItems.map((item, idx) => (
                    <tr key={`${item.tx_hash ?? ""}-${idx}`} className="hover:bg-slate-50 dark:hover:bg-slate-800/30 transition-colors">
                      <td className="px-4 py-2.5 text-slate-700 dark:text-slate-300 truncate max-w-[180px]">
                        {item.content_title}
                      </td>
                      <td className="px-4 py-2.5 font-medium text-slate-900 dark:text-slate-100">
                        {item.amount_eth} ETH
                      </td>
                      <td className="px-4 py-2.5">
                        {item.claimed ? (
                          <span className="inline-flex items-center px-2 py-0.5 rounded-full text-xs font-medium bg-slate-100 dark:bg-slate-800 text-slate-600 dark:text-slate-400">
                            Claimed
                          </span>
                        ) : (
                          <span className="inline-flex items-center px-2 py-0.5 rounded-full text-xs font-medium bg-emerald-100 dark:bg-emerald-900/30 text-emerald-700 dark:text-emerald-400">
                            Claimable
                          </span>
                        )}
                      </td>
                      <td className="px-4 py-2.5 text-slate-500 dark:text-slate-500 text-xs">
                        {fmtDate(item.distributed_at)}
                      </td>
                      <td className="px-4 py-2.5 text-xs">
                        {item.tx_hash ? (
                          <a
                            href={`https://sepolia.etherscan.io/tx/${item.tx_hash}`}
                            target="_blank"
                            rel="noopener noreferrer"
                            className="text-ara-500 hover:underline"
                          >
                            {item.tx_hash.slice(0, 8)}…
                          </a>
                        ) : (
                          <span className="text-slate-400">—</span>
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
            {(hasMore || loadingHistory) && (
              <div className="px-5 py-3 border-t border-slate-200 dark:border-slate-800 text-center">
                <button
                  onClick={handleLoadMore}
                  disabled={loadingHistory}
                  className="btn-ghost text-xs px-4 py-1.5"
                >
                  {loadingHistory ? "Loading…" : "Load More"}
                </button>
              </div>
            )}
          </div>
        </div>
      )}

      {/* Stake/Unstake Modal */}
      {showStakeModal && (
        <div className="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50 p-4">
          <div className="card w-full max-w-sm p-6 shadow-2xl">
            <h3 className="font-semibold text-slate-900 dark:text-slate-100 mb-5">
              {stakeMode === "stake" ? "Stake ARA" : "Unstake ARA"}
            </h3>
            <div className="mb-5">
              <label className="label">Amount (ARA)</label>
              <input
                type="text"
                value={stakeAmount}
                onChange={(e) => setStakeAmount(e.target.value)}
                placeholder="0.0"
                className="input-base"
                autoFocus
              />
              <p className="text-xs text-slate-500 dark:text-slate-500 mt-1.5">
                {stakeMode === "stake" ? `Available: ${araBalance} ARA` : `Staked: ${araStaked} ARA`}
              </p>
            </div>
            {stakeMode === "stake" && (
              <p className="text-xs text-slate-500 dark:text-slate-500 mb-5">
                Two transactions: (1) Approve ARA spending, then (2) Stake ARA.
              </p>
            )}
            <div className="flex justify-end gap-3">
              <button
                onClick={() => setShowStakeModal(false)}
                disabled={isSendingTx}
                className="btn-ghost"
              >
                Cancel
              </button>
              <button
                onClick={handleStake}
                disabled={isSendingTx || !stakeAmount}
                className="btn-primary"
              >
                {isSendingTx ? "Processing…" : stakeMode === "stake" ? "Stake" : "Unstake"}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

export default Wallet;
