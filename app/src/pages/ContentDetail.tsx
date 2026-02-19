import { useState, useEffect } from "react";
import { useParams, Link } from "react-router-dom";
import {
  getContentDetail,
  purchaseContent,
  confirmPurchase,
  updateContent,
  confirmUpdateContent,
  type ContentDetail as ContentDetailType,
} from "../lib/tauri";
import { signAndSendTransactions } from "../lib/transactions";
import {
  useWeb3Modal,
  useWeb3ModalAccount,
  useWeb3ModalProvider,
} from "@web3modal/ethers/react";
import { openUrl } from "@tauri-apps/plugin-opener";

type PurchaseStep = "idle" | "preparing" | "signing" | "confirming" | "done";
type EditStep = "idle" | "preparing" | "signing" | "confirming" | "done";

const STEP_LABELS: Record<PurchaseStep, string> = {
  idle: "Purchase",
  preparing: "Preparing transaction...",
  signing: "Waiting for wallet approval...",
  confirming: "Confirming purchase...",
  done: "Purchased!",
};

const EDIT_STEP_LABELS: Record<EditStep, string> = {
  idle: "Save Changes",
  preparing: "Preparing update transaction...",
  signing: "Waiting for wallet approval...",
  confirming: "Confirming update...",
  done: "Updated!",
};

function ContentDetail() {
  const { contentId } = useParams<{ contentId: string }>();
  const { open: openModal } = useWeb3Modal();
  const { isConnected, address } = useWeb3ModalAccount();
  const { walletProvider } = useWeb3ModalProvider();

  const [content, setContent] = useState<ContentDetailType | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [purchaseStep, setPurchaseStep] = useState<PurchaseStep>("idle");
  const [purchaseError, setPurchaseError] = useState<string | null>(null);
  const [purchaseTxHash, setPurchaseTxHash] = useState<string | null>(null);

  // Edit state
  const [editing, setEditing] = useState(false);
  const [editTitle, setEditTitle] = useState("");
  const [editDescription, setEditDescription] = useState("");
  const [editContentType, setEditContentType] = useState("");
  const [editPriceEth, setEditPriceEth] = useState("");
  const [editStep, setEditStep] = useState<EditStep>("idle");
  const [editError, setEditError] = useState<string | null>(null);

  useEffect(() => {
    if (!contentId) return;
    setLoading(true);
    getContentDetail(decodeURIComponent(contentId))
      .then(setContent)
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  }, [contentId]);

  const isCreator =
    isConnected &&
    address &&
    content?.creator &&
    address.toLowerCase() === content.creator.toLowerCase();

  const startEditing = () => {
    if (!content) return;
    setEditTitle(content.title);
    setEditDescription(content.description);
    setEditContentType(content.content_type || "other");
    setEditPriceEth(content.price_eth);
    setEditError(null);
    setEditStep("idle");
    setEditing(true);
  };

  const cancelEditing = () => {
    setEditing(false);
    setEditError(null);
    setEditStep("idle");
  };

  const handleUpdate = async () => {
    if (!contentId || !isConnected) return;

    setEditError(null);

    try {
      const decodedId = decodeURIComponent(contentId);

      // Step 1: Build update transaction
      setEditStep("preparing");
      const transactions = await updateContent({
        contentId: decodedId,
        title: editTitle.trim(),
        description: editDescription.trim(),
        contentType: editContentType,
        priceEth: editPriceEth.trim(),
      });

      // Step 2: Sign on-chain transaction
      if (!walletProvider) {
        openModal();
        throw new Error(
          "Wallet session expired — reconnect your wallet in the dialog that just opened, then try again."
        );
      }
      setEditStep("signing");
      await signAndSendTransactions(walletProvider, transactions);

      // Step 3: Confirm in local DB
      setEditStep("confirming");
      await confirmUpdateContent({
        contentId: decodedId,
        title: editTitle.trim(),
        description: editDescription.trim(),
        contentType: editContentType,
        priceEth: editPriceEth.trim(),
      });

      setEditStep("done");

      // Refresh content detail and exit edit mode
      const updated = await getContentDetail(decodedId);
      setContent(updated);
      setTimeout(() => {
        setEditing(false);
        setEditStep("idle");
      }, 1500);
    } catch (err) {
      setEditError(String(err));
      setEditStep("idle");
    }
  };

  const handlePurchase = async () => {
    if (!contentId || !isConnected) return;

    setPurchaseError(null);

    try {
      // Step 1: Build purchase transaction
      setPurchaseStep("preparing");
      const result = await purchaseContent(decodeURIComponent(contentId));

      // Step 2: Sign on-chain transaction (if marketplace deployed)
      let txHash = "0x0";
      if (result.transactions.length > 0) {
        if (!walletProvider) {
          openModal();
          throw new Error("Wallet session expired — reconnect your wallet in the dialog that just opened, then try again.");
        }
        setPurchaseStep("signing");
        txHash = await signAndSendTransactions(
          walletProvider,
          result.transactions
        );

        // Step 3: Record purchase in local DB
        setPurchaseStep("confirming");
        await confirmPurchase(result.content_id, txHash);
      } else {
        // Marketplace not deployed or already purchased on-chain — no tx needed
        setPurchaseStep("confirming");
        await confirmPurchase(result.content_id, txHash);
      }

      setPurchaseTxHash(txHash !== "0x0" ? txHash : null);
      setPurchaseStep("done");
    } catch (err) {
      setPurchaseError(String(err));
      setPurchaseStep("idle");
    }
  };

  const contentTypeIcon = (type: string) => {
    switch (type) {
      case "game":
        return "🎮";
      case "music":
        return "🎵";
      case "video":
        return "🎬";
      case "document":
        return "📄";
      case "software":
        return "💻";
      default:
        return "📦";
    }
  };

  if (loading) {
    return (
      <div className="text-center text-gray-400 py-12">
        Loading content...
      </div>
    );
  }

  if (error || !content) {
    return (
      <div className="max-w-2xl">
        <Link
          to="/"
          className="text-ara-600 hover:underline text-sm mb-4 inline-block"
        >
          &larr; Back to Marketplace
        </Link>
        <div className="p-4 bg-red-50 border border-red-200 rounded-lg text-red-700 text-sm">
          {error || "Content not found"}
        </div>
      </div>
    );
  }

  const canPurchase =
    isConnected && purchaseStep === "idle" && content.active && !isCreator;

  return (
    <div className="max-w-2xl">
      <Link
        to="/"
        className="text-ara-600 hover:underline text-sm mb-4 inline-block"
      >
        &larr; Back to Marketplace
      </Link>

      <div className="bg-white rounded-lg shadow-sm border border-gray-200 p-6 mt-2">
        {editing ? (
          /* ---- Edit Mode ---- */
          <div className="space-y-4">
            <h2 className="text-lg font-semibold text-gray-900">Edit Content</h2>

            <div>
              <label className="block text-sm font-medium text-gray-700 mb-1">
                Title
              </label>
              <input
                type="text"
                value={editTitle}
                onChange={(e) => setEditTitle(e.target.value)}
                disabled={editStep !== "idle"}
                className="w-full px-4 py-2 border border-gray-300 rounded-lg focus:ring-2 focus:ring-ara-500 disabled:opacity-50"
              />
            </div>

            <div>
              <label className="block text-sm font-medium text-gray-700 mb-1">
                Description
              </label>
              <textarea
                value={editDescription}
                onChange={(e) => setEditDescription(e.target.value)}
                disabled={editStep !== "idle"}
                rows={3}
                className="w-full px-4 py-2 border border-gray-300 rounded-lg focus:ring-2 focus:ring-ara-500 disabled:opacity-50"
              />
            </div>

            <div>
              <label className="block text-sm font-medium text-gray-700 mb-1">
                Content Type
              </label>
              <select
                value={editContentType}
                onChange={(e) => setEditContentType(e.target.value)}
                disabled={editStep !== "idle"}
                className="w-full px-4 py-2 border border-gray-300 rounded-lg focus:ring-2 focus:ring-ara-500 disabled:opacity-50"
              >
                <option value="game">Game</option>
                <option value="music">Music</option>
                <option value="video">Video</option>
                <option value="document">Document</option>
                <option value="software">Software</option>
                <option value="other">Other</option>
              </select>
            </div>

            <div>
              <label className="block text-sm font-medium text-gray-700 mb-1">
                Price (ETH)
              </label>
              <input
                type="text"
                value={editPriceEth}
                onChange={(e) => setEditPriceEth(e.target.value)}
                disabled={editStep !== "idle"}
                className="w-full px-4 py-2 border border-gray-300 rounded-lg focus:ring-2 focus:ring-ara-500 disabled:opacity-50"
              />
            </div>

            {editError && (
              <div className="p-3 bg-red-50 border border-red-200 rounded-lg text-red-700 text-sm">
                {editError}
              </div>
            )}

            {editStep === "done" ? (
              <div className="p-3 bg-green-50 border border-green-200 rounded-lg text-green-700 text-sm font-medium">
                Content updated successfully!
              </div>
            ) : (
              <div className="flex gap-3">
                <button
                  onClick={handleUpdate}
                  disabled={editStep !== "idle" || !editTitle.trim() || !editPriceEth.trim()}
                  className="flex-1 bg-ara-600 text-white px-4 py-2 rounded-lg font-medium hover:bg-ara-700 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  {EDIT_STEP_LABELS[editStep]}
                </button>
                <button
                  onClick={cancelEditing}
                  disabled={editStep !== "idle"}
                  className="px-4 py-2 border border-gray-300 text-gray-700 rounded-lg hover:bg-gray-50 transition-colors disabled:opacity-50"
                >
                  Cancel
                </button>
              </div>
            )}
          </div>
        ) : (
          /* ---- View Mode ---- */
          <>
            <div className="flex items-start gap-4">
              <div className="text-5xl">
                {contentTypeIcon(content.content_type)}
              </div>
              <div className="flex-1 min-w-0">
                <h1 className="text-2xl font-bold text-gray-900">
                  {content.title || "Untitled"}
                </h1>
                <p className="text-sm text-gray-500 mt-1 uppercase">
                  {content.content_type}
                </p>
              </div>
              <div className="text-right flex flex-col items-end gap-2">
                <p className="text-2xl font-bold text-ara-600">
                  {content.price_eth} ETH
                </p>
                {isCreator && (
                  <button
                    onClick={startEditing}
                    className="text-sm text-ara-600 hover:text-ara-700 font-medium"
                  >
                    Edit
                  </button>
                )}
              </div>
            </div>

            {content.description && (
              <p className="mt-4 text-gray-600">{content.description}</p>
            )}

            <div className="mt-4 grid grid-cols-2 gap-4 text-sm text-gray-500">
              <div>
                <span className="font-medium text-gray-700">Creator:</span>{" "}
                <span className="break-all text-xs">
                  {content.creator}
                </span>
              </div>
              <div>
                <span className="font-medium text-gray-700">Content Hash:</span>{" "}
                <span className="break-all text-xs">
                  {content.content_hash}
                </span>
              </div>
            </div>

            <div className="mt-6 space-y-3">
              {!isConnected && purchaseStep === "idle" && (
                <div className="p-3 bg-yellow-50 border border-yellow-200 rounded-lg text-yellow-700 text-sm">
                  Connect your wallet to purchase this content.
                </div>
              )}

              {purchaseError && (
                <div className="p-3 bg-red-50 border border-red-200 rounded-lg text-red-700 text-sm">
                  {purchaseError}
                </div>
              )}

              {purchaseStep === "done" ? (
                <div className="p-3 bg-green-50 border border-green-200 rounded-lg text-green-700 text-sm">
                  <p className="font-medium">Purchase successful!</p>
                  <p className="mt-1">
                    Check your{" "}
                    <Link
                      to="/library"
                      className="text-green-800 underline font-medium"
                    >
                      Library
                    </Link>{" "}
                    to view your content.
                    {purchaseTxHash && (
                      <>
                        {" "}
                        <button
                          onClick={() => openUrl(`https://sepolia.etherscan.io/tx/${purchaseTxHash}`)}
                          className="inline-flex items-center gap-1 text-green-800 underline font-medium cursor-pointer"
                        >
                          View on Etherscan
                          <svg className="w-3 h-3" viewBox="0 0 293.775 293.671" fill="currentColor">
                            <path d="M61.342,147.035c0-4.94,4.018-8.947,8.974-8.947h42.144c4.955,0,8.974,4.007,8.974,8.947v78.592c1.6-.56,3.6-1.2,5.8-1.92a9.157,9.157,0,0,0,6.353-8.726V120.276c0-4.945,4.014-8.952,8.97-8.952h42.16c4.955,0,8.974,4.007,8.974,8.952v85.194a9.122,9.122,0,0,0,5.611-3.043,9.157,9.157,0,0,0,2.189-5.955V93.166c0-4.94,4.018-8.947,8.974-8.947h42.144c4.955,0,8.974,4.007,8.974,8.947v87.194c0,.3-.019.592-.038.884,24.563-17.4,49.3-38.944,49.3-77.208,0-81.08-81.08-146.835-146.835-146.835S.1,22.955.1,104.035c0,62.467,43.555,105.749,82.535,131.848a9.086,9.086,0,0,0,5.1,1.557,9.161,9.161,0,0,0,9.145-9.145V147.035h-35.54Z"/>
                          </svg>
                        </button>
                      </>
                    )}
                  </p>
                </div>
              ) : isCreator ? (
                <div className="p-3 bg-blue-50 border border-blue-200 rounded-lg text-blue-700 text-sm">
                  You are the creator of this content.
                </div>
              ) : (
                <button
                  onClick={handlePurchase}
                  disabled={!canPurchase}
                  className="w-full bg-ara-600 text-white px-6 py-3 rounded-lg font-medium hover:bg-ara-700 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  {purchaseStep === "idle"
                    ? `Purchase for ${content.price_eth} ETH`
                    : STEP_LABELS[purchaseStep]}
                </button>
              )}
            </div>
          </>
        )}
      </div>
    </div>
  );
}

export default ContentDetail;
