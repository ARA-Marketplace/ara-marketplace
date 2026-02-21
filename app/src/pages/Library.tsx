import { useState, useEffect, useCallback } from "react";
import { Link } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import {
  getLibrary,
  getPublishedContent,
  startSeeding,
  stopSeeding,
  delistContent,
  confirmDelist,
  openDownloadedContent,
  openContentFolder,
  type LibraryItem,
  type PublishedItem,
} from "../lib/tauri";
import { signAndSendTransactions } from "../lib/transactions";
import { useWeb3ModalAccount, useWeb3ModalProvider } from "@web3modal/ethers/react";

type Tab = "purchased" | "published";

function formatBytes(bytes: number): string {
  if (bytes <= 0) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${units[i]}`;
}

function formatDate(ts: number): string {
  return new Date(ts * 1000).toLocaleDateString();
}

const TYPE_ICONS: Record<string, string> = {
  game: "🎮",
  music: "🎵",
  video: "🎬",
  document: "📄",
  software: "💾",
};
function typeIcon(t: string) {
  return TYPE_ICONS[t] ?? "📦";
}

function Library() {
  const { isConnected } = useWeb3ModalAccount();
  const { walletProvider } = useWeb3ModalProvider();

  const [activeTab, setActiveTab] = useState<Tab>("purchased");

  // Purchased tab state
  const [items, setItems] = useState<LibraryItem[]>([]);
  const [loadingPurchased, setLoadingPurchased] = useState(true);
  const [purchasedError, setPurchasedError] = useState<string | null>(null);
  const [togglingId, setTogglingId] = useState<string | null>(null);
  const [openingId, setOpeningId] = useState<string | null>(null);

  // Published tab state
  const [publishedItems, setPublishedItems] = useState<PublishedItem[]>([]);
  const [loadingPublished, setLoadingPublished] = useState(true);
  const [publishedError, setPublishedError] = useState<string | null>(null);
  const [delistingId, setDelistingId] = useState<string | null>(null);
  const [delistError, setDelistError] = useState<string | null>(null);
  const [pubTogglingId, setPubTogglingId] = useState<string | null>(null);

  const fetchPurchased = useCallback(() => {
    getLibrary()
      .then(setItems)
      .catch((e) => setPurchasedError(String(e)))
      .finally(() => setLoadingPurchased(false));
  }, []);

  const fetchPublished = useCallback(() => {
    getPublishedContent()
      .then(setPublishedItems)
      .catch((e) => setPublishedError(String(e)))
      .finally(() => setLoadingPublished(false));
  }, []);

  useEffect(() => {
    if (!isConnected) {
      setItems([]);
      setPublishedItems([]);
      setLoadingPurchased(false);
      setLoadingPublished(false);
      return;
    }
    fetchPurchased();
    fetchPublished();
    const interval = setInterval(() => {
      fetchPurchased();
      fetchPublished();
    }, 30000);
    return () => clearInterval(interval);
  }, [isConnected, fetchPurchased, fetchPublished]);

  // Refresh on chain sync or seeder stats changes
  useEffect(() => {
    const unlistenSync = listen("content-synced", () => {
      if (isConnected) {
        fetchPurchased();
        fetchPublished();
      }
    });
    const unlistenSeeder = listen("seeder-stats-updated", () => {
      if (isConnected) fetchPublished();
    });
    return () => {
      unlistenSync.then((f) => f());
      unlistenSeeder.then((f) => f());
    };
  }, [isConnected, fetchPurchased, fetchPublished]);

  // ── Purchased tab handlers ─────────────────────────────────────────────

  const handleToggleSeeding = async (item: LibraryItem) => {
    setTogglingId(item.content_id);
    try {
      if (item.is_seeding) {
        await stopSeeding(item.content_id);
      } else {
        await startSeeding(item.content_id);
      }
      fetchPurchased();
    } finally {
      setTogglingId(null);
    }
  };

  const handleOpenFile = async (item: LibraryItem) => {
    setOpeningId(item.content_id);
    try {
      await openDownloadedContent(item.content_id);
    } finally {
      setOpeningId(null);
    }
  };

  const handleOpenFolder = async (item: LibraryItem) => {
    setOpeningId(item.content_id);
    try {
      await openContentFolder(item.content_id);
    } finally {
      setOpeningId(null);
    }
  };

  // ── Published tab handlers ─────────────────────────────────────────────

  const handlePublishedToggleSeeding = async (item: PublishedItem) => {
    setPubTogglingId(item.content_id);
    try {
      if (item.is_seeding) {
        await stopSeeding(item.content_id);
      } else {
        await startSeeding(item.content_id);
      }
      fetchPublished();
    } finally {
      setPubTogglingId(null);
    }
  };

  const handleDelist = async (item: PublishedItem) => {
    setDelistingId(item.content_id);
    setDelistError(null);
    try {
      const txs = await delistContent(item.content_id);
      if (txs.length > 0) {
        if (!walletProvider) throw new Error("Wallet not connected");
        await signAndSendTransactions(walletProvider, txs);
      }
      await confirmDelist(item.content_id);
      fetchPublished();
    } catch (e) {
      setDelistError(String(e));
    } finally {
      setDelistingId(null);
    }
  };

  // ── Render ─────────────────────────────────────────────────────────────

  return (
    <div>
      <h1 className="text-3xl font-bold text-gray-900">Library</h1>
      <p className="mt-2 text-gray-600 mb-6">
        Manage your purchased and published content. Seed to earn ETH rewards.
      </p>

      {!isConnected && (
        <div className="p-4 bg-yellow-50 border border-yellow-200 rounded-lg text-yellow-700 text-sm mb-6">
          Connect your wallet to view your library.
        </div>
      )}

      {/* Tabs */}
      <div className="flex border-b border-gray-200 mb-6">
        <button
          onClick={() => setActiveTab("purchased")}
          className={`px-5 py-2.5 text-sm font-medium border-b-2 transition-colors ${
            activeTab === "purchased"
              ? "border-ara-600 text-ara-700"
              : "border-transparent text-gray-500 hover:text-gray-700"
          }`}
        >
          Purchased
          {items.length > 0 && (
            <span className="ml-2 text-xs bg-gray-100 text-gray-600 px-2 py-0.5 rounded-full">
              {items.length}
            </span>
          )}
        </button>
        <button
          onClick={() => setActiveTab("published")}
          className={`px-5 py-2.5 text-sm font-medium border-b-2 transition-colors ${
            activeTab === "published"
              ? "border-ara-600 text-ara-700"
              : "border-transparent text-gray-500 hover:text-gray-700"
          }`}
        >
          Published
          {publishedItems.length > 0 && (
            <span className="ml-2 text-xs bg-gray-100 text-gray-600 px-2 py-0.5 rounded-full">
              {publishedItems.length}
            </span>
          )}
        </button>
      </div>

      {/* ── Purchased Tab ───────────────────────────────────────────── */}
      {activeTab === "purchased" && (
        <>
          {purchasedError && (
            <div className="p-4 bg-red-50 border border-red-200 rounded-lg text-red-700 text-sm mb-4">
              {purchasedError}
            </div>
          )}
          {loadingPurchased ? (
            <div className="text-center text-gray-400 py-12">Loading...</div>
          ) : items.length === 0 ? (
            <div className="bg-white rounded-lg shadow-sm border border-gray-200 p-10 text-center text-gray-400">
              <p className="text-lg">No purchases yet</p>
              <p className="mt-2 text-sm">
                <Link to="/" className="text-ara-600 hover:underline">
                  Browse the Marketplace
                </Link>{" "}
                to find content to buy.
              </p>
            </div>
          ) : (
            <div className="space-y-3">
              {items.map((item) => (
                <div
                  key={item.content_id}
                  className="bg-white rounded-lg shadow-sm border border-gray-200 p-4 flex items-center gap-4"
                >
                  <span className="text-2xl flex-shrink-0">
                    {typeIcon(item.content_type)}
                  </span>
                  <div className="flex-1 min-w-0">
                    <Link
                      to={`/content/${item.content_id}`}
                      className="font-medium text-gray-900 hover:text-ara-600 truncate block"
                    >
                      {item.title || "Untitled"}
                    </Link>
                    <div className="text-xs text-gray-400 mt-0.5 flex gap-3">
                      <span>Purchased {formatDate(item.purchased_at)}</span>
                      {item.tx_hash && (
                        <a
                          href={`https://sepolia.etherscan.io/tx/${item.tx_hash}`}
                          target="_blank"
                          rel="noopener noreferrer"
                          className="text-ara-500 hover:underline"
                        >
                          Etherscan ↗
                        </a>
                      )}
                    </div>
                  </div>
                  <div className="flex items-center gap-2 flex-shrink-0">
                    {item.download_path && (
                      <>
                        <button
                          onClick={() => handleOpenFile(item)}
                          disabled={openingId === item.content_id}
                          className="text-xs px-3 py-1.5 rounded-full font-medium bg-gray-100 text-gray-700 hover:bg-gray-200 disabled:opacity-50 transition-colors"
                        >
                          Open File
                        </button>
                        <button
                          onClick={() => handleOpenFolder(item)}
                          disabled={openingId === item.content_id}
                          className="text-xs px-3 py-1.5 rounded-full font-medium bg-gray-100 text-gray-700 hover:bg-gray-200 disabled:opacity-50 transition-colors"
                        >
                          Folder
                        </button>
                      </>
                    )}
                    <button
                      onClick={() => handleToggleSeeding(item)}
                      disabled={togglingId === item.content_id}
                      className={`text-xs px-3 py-1.5 rounded-full font-medium transition-colors disabled:opacity-50 ${
                        item.is_seeding
                          ? "bg-green-100 text-green-700 hover:bg-green-200"
                          : "bg-gray-100 text-gray-600 hover:bg-gray-200"
                      }`}
                    >
                      {togglingId === item.content_id
                        ? "..."
                        : item.is_seeding
                          ? "Seeding"
                          : "Start Seeding"}
                    </button>
                  </div>
                </div>
              ))}
            </div>
          )}
        </>
      )}

      {/* ── Published Tab ───────────────────────────────────────────── */}
      {activeTab === "published" && (
        <>
          {(publishedError || delistError) && (
            <div className="p-4 bg-red-50 border border-red-200 rounded-lg text-red-700 text-sm mb-4">
              {publishedError || delistError}
            </div>
          )}
          {loadingPublished ? (
            <div className="text-center text-gray-400 py-12">Loading...</div>
          ) : publishedItems.length === 0 ? (
            <div className="bg-white rounded-lg shadow-sm border border-gray-200 p-10 text-center text-gray-400">
              <p className="text-lg">Nothing published yet</p>
              <p className="mt-2 text-sm">
                <Link to="/publish" className="text-ara-600 hover:underline">
                  Publish your first file
                </Link>{" "}
                to start earning.
              </p>
            </div>
          ) : (
            <div className="bg-white rounded-lg shadow-sm border border-gray-200 overflow-hidden">
              <table className="w-full">
                <thead className="bg-gray-50 border-b border-gray-200">
                  <tr>
                    <th className="text-left px-4 py-3 text-sm font-medium text-gray-500">
                      Title
                    </th>
                    <th className="text-right px-4 py-3 text-sm font-medium text-gray-500">
                      Size
                    </th>
                    <th className="text-right px-4 py-3 text-sm font-medium text-gray-500">
                      Price
                    </th>
                    <th className="text-right px-4 py-3 text-sm font-medium text-gray-500">
                      Seeding
                    </th>
                    <th className="text-right px-4 py-3 text-sm font-medium text-gray-500">
                      Actions
                    </th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-gray-100">
                  {publishedItems.map((item) => (
                    <tr key={item.content_id}>
                      <td className="px-4 py-3 text-sm text-gray-900">
                        <div className="flex items-center gap-2">
                          <span>{typeIcon(item.content_type)}</span>
                          <Link
                            to={`/content/${item.content_id}`}
                            className="hover:text-ara-600"
                          >
                            {item.title || "Untitled"}
                          </Link>
                        </div>
                      </td>
                      <td className="px-4 py-3 text-sm text-gray-500 text-right">
                        {formatBytes(item.file_size_bytes)}
                      </td>
                      <td className="px-4 py-3 text-sm text-gray-600 text-right">
                        {item.price_eth} ETH
                      </td>
                      <td className="px-4 py-3 text-right">
                        <button
                          onClick={() => handlePublishedToggleSeeding(item)}
                          disabled={pubTogglingId === item.content_id}
                          className={`text-xs px-3 py-1.5 rounded-full font-medium transition-colors disabled:opacity-50 ${
                            item.is_seeding
                              ? "bg-green-100 text-green-700 hover:bg-green-200"
                              : "bg-gray-100 text-gray-600 hover:bg-gray-200"
                          }`}
                        >
                          {pubTogglingId === item.content_id
                            ? "..."
                            : item.is_seeding
                              ? "Seeding"
                              : "Start Seeding"}
                        </button>
                      </td>
                      <td className="px-4 py-3 text-right">
                        <button
                          onClick={() => handleDelist(item)}
                          disabled={delistingId === item.content_id}
                          className="text-xs px-3 py-1.5 rounded-full font-medium bg-red-100 text-red-700 hover:bg-red-200 disabled:opacity-50 transition-colors"
                        >
                          {delistingId === item.content_id
                            ? "Delisting..."
                            : "Delist"}
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </>
      )}
    </div>
  );
}

export default Library;
