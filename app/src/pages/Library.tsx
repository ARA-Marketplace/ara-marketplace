import { useState, useEffect, useCallback } from "react";
import { Link } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import {
  getLibrary, getPublishedContent, startSeeding, stopSeeding,
  delistContent, confirmDelist, openDownloadedContent, openContentFolder,
  getReceiptCount,
  updateContentFile, confirmContentFileUpdate,
  type LibraryItem, type PublishedItem,
} from "../lib/tauri";
import { signAndSendTransactions } from "../lib/transactions";
import { useWeb3ModalAccount, useWeb3ModalProvider } from "@web3modal/ethers/react";

type Tab = "purchased" | "published";

function fmtBytes(bytes: number) {
  if (bytes <= 0) return "—";
  const u = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${u[i]}`;
}

function fmtDate(ts: number) {
  return new Date(ts * 1000).toLocaleDateString();
}

const TYPE_ICONS: Record<string, string> = {
  game: "🎮", music: "🎵", video: "🎬", document: "📄", software: "💾",
};
const typeIcon = (t: string) => TYPE_ICONS[t] ?? "📦";

function Library() {
  const { isConnected } = useWeb3ModalAccount();
  const { walletProvider } = useWeb3ModalProvider();
  const [activeTab, setActiveTab] = useState<Tab>("purchased");

  const [items, setItems] = useState<LibraryItem[]>([]);
  const [loadingPurchased, setLoadingPurchased] = useState(true);
  const [purchasedError, setPurchasedError] = useState<string | null>(null);
  const [togglingId, setTogglingId] = useState<string | null>(null);
  const [openingId, setOpeningId] = useState<string | null>(null);

  const [publishedItems, setPublishedItems] = useState<PublishedItem[]>([]);
  const [loadingPublished, setLoadingPublished] = useState(true);
  const [publishedError, setPublishedError] = useState<string | null>(null);
  const [delistingId, setDelistingId] = useState<string | null>(null);
  const [delistError, setDelistError] = useState<string | null>(null);
  const [pubTogglingId, setPubTogglingId] = useState<string | null>(null);
  const [rewardData, setRewardData] = useState<Record<string, { receipts: number }>>({});
  const [updatingFileId, setUpdatingFileId] = useState<string | null>(null);
  const [updateFileError, setUpdateFileError] = useState<string | null>(null);

  const fetchPurchased = useCallback(() => {
    getLibrary()
      .then(setItems)
      .catch((e) => setPurchasedError(String(e)))
      .finally(() => setLoadingPurchased(false));
  }, []);

  const fetchPublished = useCallback(() => {
    setLoadingPublished(true);
    getPublishedContent()
      .then(async (its) => {
        setPublishedItems(its);
        const entries = await Promise.all(
          its.map(async (item) => {
            const receipts = await getReceiptCount(item.content_id).catch(() => 0);
            return [item.content_id, { receipts }] as const;
          })
        );
        setRewardData(Object.fromEntries(entries));
      })
      .catch((e) => setPublishedError(String(e)))
      .finally(() => setLoadingPublished(false));
  }, []);

  useEffect(() => {
    if (!isConnected) {
      setItems([]); setPublishedItems([]);
      setLoadingPurchased(false); setLoadingPublished(false);
      return;
    }
    fetchPurchased(); fetchPublished();
    const interval = setInterval(() => { fetchPurchased(); fetchPublished(); }, 30000);
    return () => clearInterval(interval);
  }, [isConnected, fetchPurchased, fetchPublished]);

  useEffect(() => {
    const unlistenSync = listen("content-synced", () => {
      if (isConnected) { fetchPurchased(); fetchPublished(); }
    });
    const unlistenSeeder = listen("seeder-stats-updated", () => {
      if (isConnected) fetchPublished();
    });
    return () => { unlistenSync.then((f) => f()); unlistenSeeder.then((f) => f()); };
  }, [isConnected, fetchPurchased, fetchPublished]);

  const handleToggleSeeding = async (item: LibraryItem) => {
    setTogglingId(item.content_id);
    try {
      if (item.is_seeding) await stopSeeding(item.content_id);
      else await startSeeding(item.content_id);
      fetchPurchased();
    } finally { setTogglingId(null); }
  };

  const handleOpenFile = async (item: LibraryItem) => {
    setOpeningId(item.content_id);
    try { await openDownloadedContent(item.content_id); }
    finally { setOpeningId(null); }
  };

  const handleOpenFolder = async (item: LibraryItem) => {
    setOpeningId(item.content_id);
    try { await openContentFolder(item.content_id); }
    finally { setOpeningId(null); }
  };

  const handlePublishedToggleSeeding = async (item: PublishedItem) => {
    setPubTogglingId(item.content_id);
    try {
      if (item.is_seeding) await stopSeeding(item.content_id);
      else await startSeeding(item.content_id);
      fetchPublished();
    } finally { setPubTogglingId(null); }
  };

  const handleDelist = async (item: PublishedItem) => {
    setDelistingId(item.content_id); setDelistError(null);
    try {
      const txs = await delistContent(item.content_id);
      if (txs.length > 0) {
        if (!walletProvider) throw new Error("Wallet not connected");
        await signAndSendTransactions(walletProvider, txs);
      }
      await confirmDelist(item.content_id);
      fetchPublished();
    } catch (e) { setDelistError(String(e)); }
    finally { setDelistingId(null); }
  };

  const handleUpdateFile = async (item: PublishedItem) => {
    setUpdateFileError(null);
    const selected = await open({ multiple: false, directory: false, title: "Select replacement file" });
    if (!selected) return;
    setUpdatingFileId(item.content_id);
    try {
      const result = await updateContentFile({ contentId: item.content_id, filePath: selected });
      if (result.transactions.length > 0) {
        if (!walletProvider) throw new Error("Wallet not connected");
        await signAndSendTransactions(walletProvider, result.transactions);
      }
      await confirmContentFileUpdate({ contentId: item.content_id, newContentHash: result.new_content_hash });
      fetchPublished();
    } catch (e) { setUpdateFileError(String(e)); }
    finally { setUpdatingFileId(null); }
  };

  const tabCls = (t: Tab) =>
    `px-4 py-2.5 text-sm font-medium border-b-2 transition-colors ${
      activeTab === t
        ? "border-ara-600 text-ara-600 dark:text-ara-400 dark:border-ara-500"
        : "border-transparent text-slate-500 dark:text-slate-500 hover:text-slate-700 dark:hover:text-slate-300"
    }`;

  return (
    <div>
      <div className="mb-6">
        <h1 className="page-title">Library</h1>
        <p className="page-subtitle">Manage your purchased and published content.</p>
      </div>

      {!isConnected && (
        <div className="alert-warning mb-6">Connect your wallet to view your library.</div>
      )}

      {/* Tabs */}
      <div className="flex border-b border-slate-200 dark:border-slate-800 mb-6">
        <button onClick={() => setActiveTab("purchased")} className={tabCls("purchased")}>
          Purchased
          {items.length > 0 && <span className="ml-2 badge-gray">{items.length}</span>}
        </button>
        <button onClick={() => setActiveTab("published")} className={tabCls("published")}>
          Published
          {publishedItems.length > 0 && <span className="ml-2 badge-gray">{publishedItems.length}</span>}
        </button>
      </div>

      {/* ── Purchased Tab ── */}
      {activeTab === "purchased" && (
        <>
          {purchasedError && <div className="alert-error mb-4">{purchasedError}</div>}
          {loadingPurchased ? (
            <div className="flex justify-center py-16">
              <div className="w-8 h-8 border-2 border-ara-500 border-t-transparent rounded-full animate-spin" />
            </div>
          ) : items.length === 0 ? (
            <div className="card p-10 text-center">
              <p className="text-slate-400 dark:text-slate-600 mb-1">No purchases yet</p>
              <p className="text-sm text-slate-500 dark:text-slate-600">
                <Link to="/" className="text-ara-500 hover:text-ara-400 underline">Browse the Marketplace</Link>{" "}
                to find content to buy.
              </p>
            </div>
          ) : (
            <div className="space-y-2">
              {items.map((item) => (
                <div key={item.content_id}
                  className="card flex items-center gap-4 px-4 py-3.5 hover:bg-slate-50 dark:hover:bg-slate-800/40 transition-colors">
                  <span className="text-xl flex-shrink-0">{typeIcon(item.content_type)}</span>
                  <div className="flex-1 min-w-0">
                    <Link to={`/content/${item.content_id}`}
                      className="font-medium text-slate-900 dark:text-slate-100 hover:text-ara-600 dark:hover:text-ara-400 truncate block text-sm">
                      {item.title || "Untitled"}
                    </Link>
                    <div className="text-xs text-slate-400 dark:text-slate-600 mt-0.5 flex gap-3">
                      <span>Purchased {fmtDate(item.purchased_at)}</span>
                      {item.tx_hash && (
                        <a href={`https://sepolia.etherscan.io/tx/${item.tx_hash}`}
                          target="_blank" rel="noopener noreferrer"
                          className="text-ara-500 hover:underline">
                          Etherscan ↗
                        </a>
                      )}
                    </div>
                  </div>
                  <div className="flex items-center gap-2 flex-shrink-0">
                    {item.download_path && (
                      <>
                        <button onClick={() => handleOpenFile(item)}
                          disabled={openingId === item.content_id}
                          className="btn-ghost text-xs px-3 py-1.5">
                          Open File
                        </button>
                        <button onClick={() => handleOpenFolder(item)}
                          disabled={openingId === item.content_id}
                          className="btn-ghost text-xs px-3 py-1.5">
                          Folder
                        </button>
                      </>
                    )}
                    <button onClick={() => handleToggleSeeding(item)}
                      disabled={togglingId === item.content_id}
                      className={`text-xs px-3 py-1.5 rounded-lg font-medium transition-colors disabled:opacity-50 ${
                        item.is_seeding
                          ? "bg-emerald-100 dark:bg-emerald-900/20 text-emerald-700 dark:text-emerald-400 hover:bg-emerald-200 dark:hover:bg-emerald-900/40"
                          : "btn-ghost"
                      }`}>
                      {togglingId === item.content_id ? "…" : item.is_seeding ? "Seeding" : "Start Seeding"}
                    </button>
                  </div>
                </div>
              ))}
            </div>
          )}
        </>
      )}

      {/* ── Published Tab ── */}
      {activeTab === "published" && (
        <>
          <div className="flex justify-end mb-3">
            <button
              onClick={fetchPublished}
              disabled={loadingPublished}
              className="btn-ghost text-xs px-3 py-1.5"
            >
              {loadingPublished ? "Refreshing…" : "Refresh"}
            </button>
          </div>
          {(publishedError || delistError || updateFileError) && (
            <div className="alert-error mb-4">
              {publishedError || delistError || updateFileError}
            </div>
          )}
          {loadingPublished ? (
            <div className="flex justify-center py-16">
              <div className="w-8 h-8 border-2 border-ara-500 border-t-transparent rounded-full animate-spin" />
            </div>
          ) : publishedItems.length === 0 ? (
            <div className="card p-10 text-center">
              <p className="text-slate-400 dark:text-slate-600 mb-1">Nothing published yet</p>
              <p className="text-sm text-slate-500 dark:text-slate-600">
                <Link to="/publish" className="text-ara-500 hover:text-ara-400 underline">
                  Publish your first file
                </Link>{" "}
                to start earning.
              </p>
            </div>
          ) : (
            <div className="card overflow-hidden">
              <table className="w-full text-sm">
                <thead className="border-b border-slate-200 dark:border-slate-800">
                  <tr>
                    {["Title", "Size", "Price", "Rewards", "Actions"].map((h, i) => (
                      <th key={h} className={`px-4 py-3 text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-500 ${i === 0 ? "text-left" : "text-right"}`}>
                        {h}
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody className="divide-y divide-slate-100 dark:divide-slate-800/60">
                  {publishedItems.map((item) => {
                    const rd = rewardData[item.content_id];
                    return (
                      <tr key={item.content_id} className="hover:bg-slate-50 dark:hover:bg-slate-800/30 transition-colors">
                        <td className="px-4 py-3">
                          <div className="flex items-center gap-2">
                            <span>{typeIcon(item.content_type)}</span>
                            <Link to={`/content/${item.content_id}`}
                              className="text-slate-900 dark:text-slate-200 hover:text-ara-600 dark:hover:text-ara-400 font-medium">
                              {item.title || "Untitled"}
                            </Link>
                            {item.updated_at !== null && (
                              <span className="badge-blue">Updated</span>
                            )}
                          </div>
                        </td>
                        <td className="px-4 py-3 text-right text-slate-500 dark:text-slate-400">
                          {fmtBytes(item.file_size_bytes)}
                        </td>
                        <td className="px-4 py-3 text-right text-slate-600 dark:text-slate-300 font-medium">
                          {item.price_eth} ETH
                        </td>
                        <td className="px-4 py-3 text-right">
                          {rd ? (
                            <div className="text-xs text-slate-500 dark:text-slate-400">
                              <div>{rd.receipts} {rd.receipts === 1 ? "delivery" : "deliveries"}</div>
                              {rd.receipts > 0 && (
                                <Link to="/wallet" className="text-ara-500 hover:underline">
                                  Collect on Wallet
                                </Link>
                              )}
                            </div>
                          ) : (
                            <span className="text-xs text-slate-400">—</span>
                          )}
                        </td>
                        <td className="px-4 py-3">
                          <div className="flex items-center justify-end gap-1.5">
                            <button onClick={() => handlePublishedToggleSeeding(item)}
                              disabled={pubTogglingId === item.content_id}
                              className={`text-xs px-2.5 py-1.5 rounded-lg font-medium transition-colors disabled:opacity-50 ${
                                item.is_seeding
                                  ? "bg-emerald-100 dark:bg-emerald-900/20 text-emerald-700 dark:text-emerald-400 hover:bg-emerald-200 dark:hover:bg-emerald-900/40"
                                  : "btn-ghost"
                              }`}>
                              {pubTogglingId === item.content_id ? "…" : item.is_seeding ? "Seeding" : "Seed"}
                            </button>
                            <button onClick={() => handleUpdateFile(item)}
                              disabled={updatingFileId === item.content_id}
                              title="Replace the content file"
                              className="text-xs px-2.5 py-1.5 rounded-lg font-medium bg-indigo-100 dark:bg-indigo-900/20 text-indigo-700 dark:text-indigo-400 hover:bg-indigo-200 dark:hover:bg-indigo-900/40 transition-colors disabled:opacity-50">
                              {updatingFileId === item.content_id ? "…" : "Update File"}
                            </button>
                            <button onClick={() => handleDelist(item)}
                              disabled={delistingId === item.content_id}
                              className="btn-danger px-2.5 py-1.5">
                              {delistingId === item.content_id ? "…" : "Delist"}
                            </button>
                          </div>
                        </td>
                      </tr>
                    );
                  })}
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
