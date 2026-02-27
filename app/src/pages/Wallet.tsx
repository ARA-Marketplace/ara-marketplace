import { useState, useEffect, useCallback } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  useWeb3Modal,
  useWeb3ModalProvider,
} from "@web3modal/ethers/react";
import { useWalletStore } from "../store/walletStore";
import {
  getRewardHistory,
  getRewardPipeline,
  prepareClaimRewards,
  confirmClaimRewards,
  syncRewards,
  getDisplayName,
  registerName,
  confirmRegisterName,
  removeDisplayName,
  confirmRemoveName,
  type RewardHistoryItem,
  type RewardHistoryResponse,
  type RewardPipelineResponse,
} from "../lib/tauri";
import { signAndSendTransactions } from "../lib/transactions";

const PAGE_SIZE = 10;

/** Returns true if the value is a non-zero ETH string (handles null/undefined/0/0.0). */
function hasValue(v: string | undefined | null): boolean {
  if (!v) return false;
  const n = parseFloat(v);
  return !isNaN(n) && n > 0;
}

function fmtDate(ts: number) {
  // Block numbers are used as approx timestamps from sync;
  // real timestamps (from confirm commands) are unix seconds.
  if (ts > 1_000_000_000) {
    return new Date(ts * 1000).toLocaleDateString();
  }
  return `Block ${ts}`;
}

function fmtEth(val: string): string {
  const n = parseFloat(val);
  return isNaN(n) ? val : n.toFixed(3);
}

function Wallet() {
  const { open } = useWeb3Modal();
  const { walletProvider } = useWeb3ModalProvider();

  const {
    address, ethBalance, araBalance, araStaked, stakerRewardEarned,
    isLoadingBalances, isSendingTx, txStatus, error,
    refreshBalances, stakeAra, unstakeAra, claimStakingReward, clearError, clearTxStatus,
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

  // Pipeline state
  const [pipeline, setPipeline] = useState<RewardPipelineResponse | null>(null);
  const [collecting, setCollecting] = useState(false);
  const [collectStatus, setCollectStatus] = useState<string | null>(null);
  const [collectError, setCollectError] = useState<string | null>(null);

  // Display name state
  const [currentName, setCurrentName] = useState<string | null>(null);
  const [nameInput, setNameInput] = useState("");
  const [nameStep, setNameStep] = useState<"idle" | "signing" | "confirming" | "done">("idle");
  const [nameError, setNameError] = useState<string | null>(null);

  const fetchPipeline = useCallback(async () => {
    try {
      const data = await getRewardPipeline();
      setPipeline(data);
    } catch (e) {
      console.warn("Failed to fetch reward pipeline:", e);
    }
  }, []);

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

  // Fetch current display name
  useEffect(() => {
    if (address) {
      getDisplayName(address).then((name) => {
        setCurrentName(name);
        if (name) setNameInput(name);
      }).catch(() => {});
    } else {
      setCurrentName(null);
      setNameInput("");
    }
  }, [address]);

  const handleSetName = async () => {
    if (!walletProvider || !nameInput.trim()) return;
    setNameError(null);
    try {
      setNameStep("signing");
      const txs = await registerName(nameInput.trim());
      await signAndSendTransactions(walletProvider, txs);
      setNameStep("confirming");
      await confirmRegisterName(nameInput.trim());
      setCurrentName(nameInput.trim());
      setNameStep("done");
      setTimeout(() => setNameStep("idle"), 1500);
    } catch (e) {
      setNameError(String(e));
      setNameStep("idle");
    }
  };

  const handleRemoveName = async () => {
    if (!walletProvider) return;
    setNameError(null);
    try {
      setNameStep("signing");
      const txs = await removeDisplayName();
      await signAndSendTransactions(walletProvider, txs);
      setNameStep("confirming");
      await confirmRemoveName();
      setCurrentName(null);
      setNameInput("");
      setNameStep("done");
      setTimeout(() => setNameStep("idle"), 1500);
    } catch (e) {
      setNameError(String(e));
      setNameStep("idle");
    }
  };

  // Refresh balances, pipeline, and history on every mount (handles navigation from other pages).
  // Sync rewards from chain first to ensure DB has the latest events.
  useEffect(() => {
    if (address) {
      refreshBalances();
      fetchPipeline();
      setHistoryOffset(0);
      // Sync from chain, then fetch history (so newly claimed rewards appear)
      syncRewards()
        .catch(() => {})
        .finally(() => fetchRewardHistory(0, false));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Auto-refresh when background reward sync finds new events
  useEffect(() => {
    const unlisten = listen("rewards-synced", () => {
      if (address) {
        refreshBalances();
        fetchPipeline();
        fetchRewardHistory(0, false);
        setHistoryOffset(0);
      }
    });
    return () => { unlisten.then((f) => f()); };
  }, [address, refreshBalances, fetchPipeline, fetchRewardHistory]);

  // Periodic refresh: pipeline queries on-chain data, so poll every 30s to catch
  // purchases of creator's content (which the background sync doesn't detect).
  useEffect(() => {
    if (!address) return;
    const interval = setInterval(() => {
      refreshBalances();
      fetchPipeline();
    }, 30_000);
    return () => clearInterval(interval);
  }, [address, refreshBalances, fetchPipeline]);

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

  const handleClaimStakingReward = async () => {
    if (!walletProvider) { open(); return; }
    try {
      await claimStakingReward(walletProvider);
    } catch { /* error set in store */ }
  };

  const handleCollect = async () => {
    if (!walletProvider) { open(); return; }
    setCollecting(true);
    setCollectError(null);
    setCollectStatus("Preparing claim transaction...");
    try {
      const txs = await prepareClaimRewards();
      const lastTxHash = await signAndSendTransactions(
        walletProvider,
        txs,
        (msg) => setCollectStatus(msg),
      );
      // Record the claim in local DB
      setCollectStatus("Recording on chain...");
      await confirmClaimRewards(lastTxHash).catch((e) => {
        console.warn("confirm_claim_rewards failed, will rely on sync:", e);
      });
      // Re-sync to capture all claim events
      await syncRewards().catch(() => {});
      setCollectStatus(null);
      // Refresh everything
      await Promise.all([
        refreshBalances(),
        fetchPipeline(),
        fetchRewardHistory(0, false),
      ]);
      setHistoryOffset(0);
    } catch (e) {
      setCollectError(String(e));
      setCollectStatus(null);
    } finally {
      setCollecting(false);
    }
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
            Connect Wallet
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

          {/* Display Name */}
          <div className="card p-5">
            <p className="text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-500 mb-3">
              Display Name
            </p>
            {currentName && (
              <p className="text-sm text-slate-700 dark:text-slate-300 mb-2">
                Currently: <span className="font-medium">{currentName}</span>
              </p>
            )}
            {nameError && (
              <div className="alert-error text-xs mb-2">{nameError}</div>
            )}
            <div className="flex gap-2">
              <input
                type="text"
                value={nameInput}
                onChange={(e) => setNameInput(e.target.value)}
                placeholder="Choose a display name"
                className="input-base flex-1"
                maxLength={32}
              />
              <button
                onClick={handleSetName}
                disabled={nameStep !== "idle" || !nameInput.trim()}
                className="btn-primary text-sm px-4"
              >
                {nameStep === "signing" ? "Sign..." : nameStep === "confirming" ? "Confirming..." : nameStep === "done" ? "Set!" : "Set Name"}
              </button>
              {currentName && (
                <button
                  onClick={handleRemoveName}
                  disabled={nameStep !== "idle"}
                  className="btn-danger text-sm px-3"
                >
                  Remove
                </button>
              )}
            </div>
            <p className="text-[10px] text-slate-400 dark:text-slate-600 mt-1.5">
              1-32 characters. Alphanumeric, hyphens, and underscores only. Stored on-chain.
            </p>
          </div>

          {/* Balances */}
          <div className="grid grid-cols-2 gap-4">
            {[
              { label: "ETH Balance",  value: `${fmtEth(ethBalance)} ETH` },
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

          {/* Staking Rewards */}
          {hasValue(stakerRewardEarned) && (
            <div className="card p-5">
              <div className="flex justify-between items-center">
                <div>
                  <p className="text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-500 mb-0.5">
                    Staking Rewards
                  </p>
                  <p className="text-[10px] text-slate-400 dark:text-slate-600 mb-2">
                    Earned from staking ARA (2.5% of purchases)
                  </p>
                  <p className="text-2xl font-bold text-emerald-600 dark:text-emerald-400">
                    {fmtEth(stakerRewardEarned)} <span className="text-sm font-normal text-slate-500">ETH</span>
                  </p>
                </div>
                <div className="flex-shrink-0 ml-4">
                  <button
                    onClick={handleClaimStakingReward}
                    disabled={isSendingTx}
                    className="btn-success px-5 py-2 text-sm"
                  >
                    {isSendingTx ? "Claiming..." : "Claim"}
                  </button>
                </div>
              </div>
            </div>
          )}

          {/* Rewards: Ready to Collect + Lifetime Earnings */}
          {collectError && (
            <div className="alert-error mb-0 flex justify-between items-center">
              <span>{collectError}</span>
              <button onClick={() => setCollectError(null)} className="text-xs font-medium ml-4 hover:opacity-70 flex-shrink-0">
                Dismiss
              </button>
            </div>
          )}
          {collectStatus && (
            <div className="alert-info mb-0 flex items-center">
              <span>{collectStatus}</span>
            </div>
          )}

          {/* Ready to Collect */}
          <div className="card p-5">
            <div className="flex justify-between items-center">
              <div>
                <p className="text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-500 mb-0.5">
                  Ready to Collect
                </p>
                <p className="text-[10px] text-slate-400 dark:text-slate-600 mb-2">
                  Rewards earned from seeding
                </p>
                <p className="text-2xl font-bold text-emerald-600 dark:text-emerald-400">
                  {pipeline?.available_eth ?? "0.0"} <span className="text-sm font-normal text-slate-500">ETH</span>
                </p>
                {pipeline && pipeline.receipt_count > 0 && (
                  <p className="text-[10px] text-slate-400 dark:text-slate-600 mt-1">
                    {pipeline.receipt_count} {pipeline.receipt_count === 1 ? "delivery" : "deliveries"}
                  </p>
                )}
              </div>
              <div className="flex-shrink-0 ml-4">
                <button
                  onClick={handleCollect}
                  disabled={collecting || isSendingTx || !pipeline || !hasValue(pipeline?.available_eth)}
                  className="btn-success px-5 py-2 text-sm"
                >
                  {collecting ? "Collecting..." : "Collect All"}
                </button>
              </div>
            </div>
          </div>

          {/* Lifetime Earnings */}
          <div className="card p-5">
            <p className="text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-500 mb-1">
              Lifetime Earnings
            </p>
            <p className="text-[10px] text-slate-400 dark:text-slate-600 mb-2">
              Total rewards earned
            </p>
            <p className="text-2xl font-bold text-slate-900 dark:text-slate-100">
              {pipeline?.lifetime_earnings_eth ?? rewardHistory?.total_earned_eth ?? "0.0"} <span className="text-sm font-normal text-slate-500">ETH</span>
            </p>
          </div>

          {/* How Rewards Work */}
          <div className="card p-5">
            <p className="text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-500 mb-3">
              How Rewards Work
            </p>
            <div className="text-xs text-slate-600 dark:text-slate-400 space-y-1.5">
              <p>When buyers purchase content, the price is split: <span className="font-medium text-slate-700 dark:text-slate-300">85% to the creator</span>, <span className="font-medium text-slate-700 dark:text-slate-300">2.5% to ARA stakers</span>, and <span className="font-medium text-slate-700 dark:text-slate-300">12.5% to seeders</span>.</p>
              <p>Staking rewards are <span className="font-medium text-slate-700 dark:text-slate-300">proportional to your stake</span> — the more ARA you stake, the larger your share of the 2.5%. They accrue automatically with every purchase and can be claimed anytime.</p>
              <p>Resale purchases split similarly: 4% to seeders, 1% to stakers (plus creator royalties).</p>
              <p>Seeders who deliver content earn signed delivery receipts. Click <span className="font-medium text-slate-700 dark:text-slate-300">Collect All</span> to claim seeder rewards.</p>
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
                No reward events yet. Rewards appear here after you collect.
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
