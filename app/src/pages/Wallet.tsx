import { useState } from "react";
import {
  useWeb3Modal,
  useWeb3ModalAccount,
  useWeb3ModalProvider,
} from "@web3modal/ethers/react";
import { useWalletStore } from "../store/walletStore";

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
    try { await claimRewards(walletProvider); }
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

          {/* Rewards */}
          <div className="card p-5">
            <p className="text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-500 mb-4">
              Rewards
            </p>
            <div className="flex justify-between items-center">
              <div>
                <p className="text-2xl font-bold text-emerald-600 dark:text-emerald-400">
                  {claimableRewards} <span className="text-sm font-normal text-slate-500">ETH</span>
                </p>
                <p className="text-xs text-slate-500 dark:text-slate-500 mt-0.5">
                  Earn ETH by seeding content you own.
                </p>
              </div>
              <button
                onClick={handleClaim}
                disabled={isSendingTx || claimableRewards === "0" || claimableRewards === "0.0"}
                className="btn-success flex-shrink-0 ml-4 px-4 py-2 text-sm"
              >
                Claim Rewards
              </button>
            </div>
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
