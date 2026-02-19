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
    address,
    ethBalance,
    araBalance,
    araStaked,
    claimableRewards,
    isLoadingBalances,
    isSendingTx,
    txStatus,
    error,
    refreshBalances,
    stakeAra,
    unstakeAra,
    claimRewards,
    clearError,
    clearTxStatus,
  } = useWalletStore();

  // Stake/Unstake modal state
  const [showStakeModal, setShowStakeModal] = useState(false);
  const [stakeMode, setStakeMode] = useState<"stake" | "unstake">("stake");
  const [stakeAmount, setStakeAmount] = useState("");

  const handleStake = async () => {
    if (!stakeAmount) return;
    if (!walletProvider) {
      // Web3Modal provider is stale — ask user to reconnect
      open();
      return;
    }
    try {
      if (stakeMode === "stake") {
        await stakeAra(stakeAmount, walletProvider);
      } else {
        await unstakeAra(stakeAmount, walletProvider);
      }
      setShowStakeModal(false);
      setStakeAmount("");
    } catch {
      // Error is already set in store
    }
  };

  const handleClaim = async () => {
    if (!walletProvider) {
      open();
      return;
    }
    try {
      await claimRewards(walletProvider);
    } catch {
      // Error is already set in store
    }
  };

  return (
    <div className="max-w-2xl">
      <h1 className="text-3xl font-bold text-gray-900">Wallet</h1>
      <p className="mt-2 text-gray-600 mb-8">
        Manage your ARA tokens, ETH, and staking.
      </p>

      {/* Error banner */}
      {error && (
        <div className="mb-4 bg-red-50 border border-red-200 rounded-lg p-4 flex justify-between items-center">
          <p className="text-sm text-red-700">{error}</p>
          <button
            onClick={clearError}
            className="text-red-500 hover:text-red-700 text-sm font-medium"
          >
            Dismiss
          </button>
        </div>
      )}

      {/* Transaction status banner */}
      {txStatus && (
        <div className="mb-4 bg-blue-50 border border-blue-200 rounded-lg p-4 flex justify-between items-center">
          <p className="text-sm text-blue-700">{txStatus}</p>
          {!isSendingTx && (
            <button
              onClick={clearTxStatus}
              className="text-blue-500 hover:text-blue-700 text-sm font-medium"
            >
              Dismiss
            </button>
          )}
        </div>
      )}

      {!address ? (
        <div className="bg-white rounded-lg shadow-sm border border-gray-200 p-8 text-center">
          <p className="text-gray-600 mb-4">
            Connect your wallet to manage tokens and staking.
          </p>
          <button
            onClick={() => open()}
            className="bg-ara-600 text-white px-6 py-3 rounded-lg font-medium hover:bg-ara-700 transition-colors"
          >
            {isConnected ? "Switch Wallet" : "Connect Wallet"}
          </button>
        </div>
      ) : (
        <div className="space-y-6">
          {/* Connected address */}
          <div className="bg-white rounded-lg shadow-sm border border-gray-200 p-6">
            <div className="flex justify-between items-center">
              <div>
                <p className="text-sm text-gray-500">Connected Address</p>
                <p className="font-mono text-gray-900 mt-1">{address}</p>
              </div>
              <button
                onClick={refreshBalances}
                disabled={isLoadingBalances}
                className="text-ara-600 hover:text-ara-700 text-sm font-medium disabled:opacity-50"
              >
                {isLoadingBalances ? "Refreshing..." : "Refresh"}
              </button>
            </div>
          </div>

          {/* Balances grid */}
          <div className="grid grid-cols-2 gap-4">
            <div className="bg-white rounded-lg shadow-sm border border-gray-200 p-6">
              <p className="text-sm text-gray-500">ETH Balance</p>
              <p className="text-xl font-bold text-gray-900 mt-1">
                {ethBalance} ETH
              </p>
            </div>
            <div className="bg-white rounded-lg shadow-sm border border-gray-200 p-6">
              <p className="text-sm text-gray-500">ARA Balance</p>
              <p className="text-xl font-bold text-gray-900 mt-1">
                {araBalance} ARA
              </p>
            </div>
          </div>

          {/* Staking section */}
          <div className="bg-white rounded-lg shadow-sm border border-gray-200 p-6">
            <h2 className="text-lg font-semibold text-gray-900 mb-4">
              Staking
            </h2>
            <div className="flex justify-between items-center mb-4">
              <div>
                <p className="text-sm text-gray-500">ARA Staked</p>
                <p className="text-xl font-bold text-gray-900">
                  {araStaked} ARA
                </p>
              </div>
              <div className="space-x-2">
                <button
                  onClick={() => {
                    setStakeMode("stake");
                    setStakeAmount("");
                    setShowStakeModal(true);
                  }}
                  disabled={isSendingTx}
                  className="bg-ara-600 text-white px-4 py-2 rounded-lg text-sm font-medium hover:bg-ara-700 disabled:opacity-50"
                >
                  Stake
                </button>
                <button
                  onClick={() => {
                    setStakeMode("unstake");
                    setStakeAmount("");
                    setShowStakeModal(true);
                  }}
                  disabled={isSendingTx}
                  className="bg-gray-200 text-gray-700 px-4 py-2 rounded-lg text-sm font-medium hover:bg-gray-300 disabled:opacity-50"
                >
                  Unstake
                </button>
              </div>
            </div>
            <p className="text-xs text-gray-400">
              Stake ARA to become eligible to publish content and seed for
              rewards.
            </p>
          </div>

          {/* Rewards section */}
          <div className="bg-white rounded-lg shadow-sm border border-gray-200 p-6">
            <h2 className="text-lg font-semibold text-gray-900 mb-4">
              Rewards
            </h2>
            <div className="flex justify-between items-center">
              <div>
                <p className="text-sm text-gray-500">Claimable ETH Rewards</p>
                <p className="text-xl font-bold text-green-600">
                  {claimableRewards} ETH
                </p>
              </div>
              <button
                onClick={handleClaim}
                disabled={isSendingTx || claimableRewards === "0" || claimableRewards === "0.0"}
                className="bg-green-600 text-white px-4 py-2 rounded-lg text-sm font-medium hover:bg-green-700 disabled:opacity-50"
              >
                Claim Rewards
              </button>
            </div>
            <p className="text-xs text-gray-400 mt-2">
              Earn ETH by seeding content. Rewards accumulate as buyers purchase
              content you seed.
            </p>
          </div>
        </div>
      )}

      {/* Stake/Unstake Modal */}
      {showStakeModal && (
        <div className="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50">
          <div className="bg-white rounded-xl shadow-xl p-6 w-full max-w-md">
            <h3 className="text-lg font-semibold text-gray-900 mb-4">
              {stakeMode === "stake" ? "Stake ARA" : "Unstake ARA"}
            </h3>

            <div className="mb-4">
              <label className="block text-sm text-gray-600 mb-1">
                Amount (ARA)
              </label>
              <input
                type="text"
                value={stakeAmount}
                onChange={(e) => setStakeAmount(e.target.value)}
                placeholder="0.0"
                className="w-full border border-gray-300 rounded-lg px-4 py-2 text-gray-900 focus:outline-none focus:ring-2 focus:ring-ara-500 focus:border-transparent"
              />
              <p className="text-xs text-gray-400 mt-1">
                {stakeMode === "stake"
                  ? `Available: ${araBalance} ARA`
                  : `Staked: ${araStaked} ARA`}
              </p>
            </div>

            {stakeMode === "stake" && (
              <p className="text-xs text-gray-500 mb-4">
                This will send two transactions: (1) Approve ARA spending, then
                (2) Stake ARA.
              </p>
            )}

            <div className="flex justify-end space-x-3">
              <button
                onClick={() => setShowStakeModal(false)}
                disabled={isSendingTx}
                className="px-4 py-2 text-sm text-gray-600 hover:text-gray-800 disabled:opacity-50"
              >
                Cancel
              </button>
              <button
                onClick={handleStake}
                disabled={isSendingTx || !stakeAmount}
                className={`px-4 py-2 text-sm font-medium rounded-lg text-white disabled:opacity-50 ${
                  stakeMode === "stake"
                    ? "bg-ara-600 hover:bg-ara-700"
                    : "bg-gray-600 hover:bg-gray-700"
                }`}
              >
                {isSendingTx
                  ? "Processing..."
                  : stakeMode === "stake"
                  ? "Stake"
                  : "Unstake"}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

export default Wallet;
