import { useState, useCallback, useEffect, type DragEvent } from "react";
import { useNavigate } from "react-router-dom";
import { open } from "@tauri-apps/plugin-dialog";
import {
  publishContent,
  confirmPublish,
  getMyContent,
  delistContent,
  confirmDelist,
  type ContentDetail,
} from "../lib/tauri";
import { signAndSendTransactions } from "../lib/transactions";
import {
  useWeb3Modal,
  useWeb3ModalAccount,
  useWeb3ModalProvider,
} from "@web3modal/ethers/react";

type PublishStep = "form" | "importing" | "signing" | "confirming" | "done";

const STEP_LABELS: Record<PublishStep, string> = {
  form: "Publish Content",
  importing: "Importing file into P2P network...",
  signing: "Waiting for wallet approval...",
  confirming: "Confirming and activating content...",
  done: "Published!",
};

function Publish() {
  const navigate = useNavigate();
  const { open: openModal } = useWeb3Modal();
  const { isConnected } = useWeb3ModalAccount();
  const { walletProvider } = useWeb3ModalProvider();

  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  const [priceEth, setPriceEth] = useState("");
  const [contentType, setContentType] = useState("game");
  const [filePath, setFilePath] = useState<string | null>(null);
  const [isDragging, setIsDragging] = useState(false);
  const [step, setStep] = useState<PublishStep>("form");
  const [error, setError] = useState<string | null>(null);
  const [resultHash, setResultHash] = useState<string | null>(null);

  // My Content panel
  const [myContent, setMyContent] = useState<ContentDetail[]>([]);
  const [delistingId, setDelistingId] = useState<string | null>(null);
  const [delistError, setDelistError] = useState<string | null>(null);

  const fetchMyContent = useCallback(() => {
    if (!isConnected) { setMyContent([]); return; }
    getMyContent().then(setMyContent).catch(() => {});
  }, [isConnected]);

  useEffect(() => { fetchMyContent(); }, [fetchMyContent]);

  const handleDelist = async (item: ContentDetail) => {
    setDelistingId(item.content_id);
    setDelistError(null);
    try {
      const txs = await delistContent(item.content_id);
      if (txs.length > 0) {
        if (!walletProvider) throw new Error("Wallet not connected");
        await signAndSendTransactions(walletProvider, txs);
      }
      await confirmDelist(item.content_id);
      fetchMyContent();
    } catch (e) {
      setDelistError(String(e));
    } finally {
      setDelistingId(null);
    }
  };

  const selectFile = async () => {
    const selected = await open({
      multiple: false,
      directory: false,
      title: "Select content file",
    });
    if (selected) {
      setFilePath(selected);
    }
  };

  const handleDragOver = useCallback((e: DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragging(true);
  }, []);

  const handleDragLeave = useCallback((e: DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragging(false);
  }, []);

  const handleDrop = useCallback((e: DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragging(false);

    const files = e.dataTransfer.files;
    if (files.length > 0) {
      const file = files[0];
      const path = (file as unknown as { path?: string }).path;
      if (path) {
        setFilePath(path);
      }
    }
  }, []);

  const fileName = filePath
    ? filePath.split(/[\\/]/).pop() ?? filePath
    : null;

  const canPublish =
    filePath &&
    title.trim() &&
    priceEth.trim() &&
    step === "form" &&
    isConnected;

  const handlePublish = async () => {
    if (!filePath || !title.trim() || !priceEth.trim()) return;

    setError(null);
    setResultHash(null);

    try {
      // Step 1: Import file into iroh + build transaction
      setStep("importing");
      const result = await publishContent({
        filePath,
        title: title.trim(),
        description: description.trim(),
        contentType,
        priceEth: priceEth.trim(),
      });

      setResultHash(result.content_hash);

      // Step 2: Sign on-chain transaction (if registry is deployed)
      if (result.transactions.length > 0) {
        if (!walletProvider) {
          // Session is stale — open Web3Modal to re-establish the provider, then user retries
          openModal();
          throw new Error("Wallet session expired — reconnect your wallet in the dialog that just opened, then publish again.");
        }
        setStep("signing");
        const txHash = await signAndSendTransactions(
          walletProvider,
          result.transactions
        );

        // Step 3: Confirm and start seeding
        setStep("confirming");
        await confirmPublish(result.content_hash, txHash);
      } else {
        // Registry not deployed or content already confirmed on-chain — no tx needed
        setStep("confirming");
        await confirmPublish(result.content_hash, "0x0");
      }

      setStep("done");

      // Navigate to Marketplace after a short delay so the user sees "Published!"
      // This also forces a remount → the Marketplace refetches and shows the new content.
      setTimeout(() => {
        navigate("/");
      }, 1500);
    } catch (err) {
      setError(String(err));
      setStep("form");
    }
  };

  const resetForm = () => {
    setStep("form");
    setResultHash(null);
    setError(null);
  };

  return (
    <div className="max-w-2xl">
      <h1 className="text-3xl font-bold text-gray-900">Publish Content</h1>
      <p className="mt-2 text-gray-600 mb-8">
        Share your content with the world. You'll earn 85% of every purchase in
        ETH.
      </p>

      <div className="space-y-6">
        <div>
          <label className="block text-sm font-medium text-gray-700 mb-1">
            Title
          </label>
          <input
            type="text"
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            disabled={step !== "form"}
            className="w-full px-4 py-2 border border-gray-300 rounded-lg focus:ring-2 focus:ring-ara-500 disabled:opacity-50"
            placeholder="My Awesome Game"
          />
        </div>

        <div>
          <label className="block text-sm font-medium text-gray-700 mb-1">
            Description
          </label>
          <textarea
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            disabled={step !== "form"}
            rows={4}
            className="w-full px-4 py-2 border border-gray-300 rounded-lg focus:ring-2 focus:ring-ara-500 disabled:opacity-50"
            placeholder="Describe your content..."
          />
        </div>

        <div>
          <label className="block text-sm font-medium text-gray-700 mb-1">
            Content Type
          </label>
          <select
            value={contentType}
            onChange={(e) => setContentType(e.target.value)}
            disabled={step !== "form"}
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
            value={priceEth}
            onChange={(e) => setPriceEth(e.target.value)}
            disabled={step !== "form"}
            className="w-full px-4 py-2 border border-gray-300 rounded-lg focus:ring-2 focus:ring-ara-500 disabled:opacity-50"
            placeholder="0.1"
          />
        </div>

        <div>
          <label className="block text-sm font-medium text-gray-700 mb-1">
            File
          </label>
          <button
            onClick={selectFile}
            onDragOver={handleDragOver}
            onDragLeave={handleDragLeave}
            onDrop={handleDrop}
            disabled={step !== "form"}
            className={`w-full px-4 py-8 border-2 border-dashed rounded-lg transition-colors disabled:opacity-50 ${
              isDragging
                ? "border-ara-500 bg-ara-50 text-ara-600"
                : filePath
                  ? "border-ara-500 text-ara-700"
                  : "border-gray-300 text-gray-400 hover:border-ara-500 hover:text-ara-600"
            }`}
          >
            {filePath ? (
              <span className="flex flex-col items-center gap-1">
                <span className="font-medium">{fileName}</span>
                <span className="text-sm text-gray-400">
                  Click to change file
                </span>
              </span>
            ) : (
              "Click to select file or drag and drop"
            )}
          </button>
        </div>

        {error && (
          <div className="p-4 bg-red-50 border border-red-200 rounded-lg text-red-700 text-sm">
            {error}
          </div>
        )}

        {!isConnected && step === "form" && (
          <div className="p-4 bg-yellow-50 border border-yellow-200 rounded-lg text-yellow-700 text-sm">
            Connect your wallet to publish content.
          </div>
        )}

        {step === "done" && resultHash && (
          <div className="p-4 bg-green-50 border border-green-200 rounded-lg text-green-700 text-sm">
            <p className="font-medium">Published successfully!</p>
            <p className="mt-1 break-all text-xs">
              Content hash: {resultHash}
            </p>
          </div>
        )}

        {step === "done" ? (
          <button
            onClick={resetForm}
            className="w-full bg-ara-600 text-white px-6 py-3 rounded-lg font-medium hover:bg-ara-700 transition-colors"
          >
            Publish Another
          </button>
        ) : (
          <button
            onClick={handlePublish}
            disabled={!canPublish}
            className="w-full bg-ara-600 text-white px-6 py-3 rounded-lg font-medium hover:bg-ara-700 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          >
            {STEP_LABELS[step]}
          </button>
        )}
      </div>
      {isConnected && myContent.length > 0 && (
        <div className="mt-12">
          <h2 className="text-xl font-bold text-gray-900 mb-4">My Published Content</h2>
          {delistError && (
            <div className="p-3 mb-4 bg-red-50 border border-red-200 rounded-lg text-red-700 text-sm">
              {delistError}
            </div>
          )}
          <div className="bg-white rounded-lg shadow-sm border border-gray-200 overflow-hidden">
            <table className="w-full">
              <thead className="bg-gray-50 border-b border-gray-200">
                <tr>
                  <th className="text-left px-4 py-3 text-sm font-medium text-gray-500">Title</th>
                  <th className="text-right px-4 py-3 text-sm font-medium text-gray-500">Price</th>
                  <th className="text-right px-4 py-3 text-sm font-medium text-gray-500">Status</th>
                  <th className="text-right px-4 py-3 text-sm font-medium text-gray-500"></th>
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-100">
                {myContent.map((item) => (
                  <tr key={item.content_id}>
                    <td className="px-4 py-3 text-sm text-gray-900">{item.title || "Untitled"}</td>
                    <td className="px-4 py-3 text-sm text-gray-600 text-right">{item.price_eth} ETH</td>
                    <td className="px-4 py-3 text-right">
                      <span className={`text-xs px-2 py-1 rounded-full ${item.active ? "bg-green-100 text-green-700" : "bg-gray-100 text-gray-500"}`}>
                        {item.active ? "Active" : "Delisted"}
                      </span>
                    </td>
                    <td className="px-4 py-3 text-right">
                      {item.active && (
                        <button
                          onClick={() => handleDelist(item)}
                          disabled={delistingId === item.content_id}
                          className="text-sm px-3 py-1.5 rounded-full font-medium transition-colors bg-red-100 text-red-700 hover:bg-red-200 disabled:opacity-50"
                        >
                          {delistingId === item.content_id ? "Delisting..." : "Delist"}
                        </button>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}
    </div>
  );
}

export default Publish;
