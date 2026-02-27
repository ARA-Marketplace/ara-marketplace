import { useState, useEffect, useCallback } from "react";
import { Link } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import {
  getLibrary, getPublishedContent, startSeeding, stopSeeding,
  delistContent, confirmDelist, openDownloadedContent, openContentFolder,
  getReceiptCount,
  updateContentFile, confirmContentFileUpdate,
  listForResale, confirmListForResale,
  cancelResaleListing, confirmCancelListing,
  getResaleListings,
  getMyCollections, createCollection, confirmCreateCollection,
  type LibraryItem, type PublishedItem, type ResaleListing, type CollectionInfo,
} from "../lib/tauri";
import { signAndSendTransactions } from "../lib/transactions";
import { useWeb3Modal, useWeb3ModalAccount, useWeb3ModalProvider } from "@web3modal/ethers/react";

type Tab = "purchased" | "published" | "collections";

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
  const { open: openModal } = useWeb3Modal();
  const { isConnected, address } = useWeb3ModalAccount();
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

  // Resale state
  const [activeListings, setActiveListings] = useState<Record<string, ResaleListing>>({});
  const [resaleModalItem, setResaleModalItem] = useState<LibraryItem | null>(null);
  const [resalePrice, setResalePrice] = useState("");
  const [resaleStep, setResaleStep] = useState<"idle" | "signing" | "confirming">("idle");
  const [resaleError, setResaleError] = useState<string | null>(null);
  const [resaleStatus, setResaleStatus] = useState<string | null>(null);
  const [cancellingId, setCancellingId] = useState<string | null>(null);

  // Collections state
  const [collections, setCollections] = useState<CollectionInfo[]>([]);
  const [loadingCollections, setLoadingCollections] = useState(false);
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [newCollName, setNewCollName] = useState("");
  const [newCollDesc, setNewCollDesc] = useState("");
  const [newCollBanner, setNewCollBanner] = useState("");
  const [createStep, setCreateStep] = useState<"idle" | "signing" | "confirming" | "done">("idle");
  const [createError, setCreateError] = useState<string | null>(null);

  const fetchCollections = useCallback(async () => {
    if (!isConnected) return;
    setLoadingCollections(true);
    try {
      const data = await getMyCollections();
      setCollections(data);
    } catch {}
    finally { setLoadingCollections(false); }
  }, [isConnected]);

  useEffect(() => { fetchCollections(); }, [fetchCollections]);

  const handleCreateCollection = async () => {
    if (!walletProvider || !newCollName.trim()) return;
    setCreateError(null);
    try {
      setCreateStep("signing");
      const txs = await createCollection({
        name: newCollName.trim(),
        description: newCollDesc,
        bannerUri: newCollBanner,
      });
      const { signAndSendTransactions: sign } = await import("../lib/transactions");
      const txHash = await sign(walletProvider, txs);
      setCreateStep("confirming");
      await confirmCreateCollection({
        txHash,
        name: newCollName.trim(),
        description: newCollDesc,
        bannerUri: newCollBanner,
      });
      setCreateStep("done");
      setShowCreateModal(false);
      setNewCollName(""); setNewCollDesc(""); setNewCollBanner("");
      fetchCollections();
      setTimeout(() => setCreateStep("idle"), 1500);
    } catch (e) {
      setCreateError(String(e));
      setCreateStep("idle");
    }
  };

  const fetchActiveListings = useCallback(async (libraryItems: LibraryItem[]) => {
    if (!address) return;
    const entries = await Promise.all(
      libraryItems.map(async (item) => {
        try {
          const listings = await getResaleListings(item.content_id);
          const mine = listings.find((l) => l.seller.toLowerCase() === address.toLowerCase());
          return mine ? [item.content_id, mine] as const : null;
        } catch { return null; }
      })
    );
    const record: Record<string, ResaleListing> = {};
    for (const e of entries) { if (e) record[e[0]] = e[1]; }
    setActiveListings(record);
  }, [address]);

  const fetchPurchased = useCallback(() => {
    getLibrary()
      .then((its) => { setItems(its); fetchActiveListings(its); })
      .catch((e) => setPurchasedError(String(e)))
      .finally(() => setLoadingPurchased(false));
  }, [fetchActiveListings]);

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

  const handleListForResale = async () => {
    if (!resaleModalItem || !resalePrice.trim()) return;
    setResaleError(null);
    setResaleStatus(null);
    try {
      setResaleStep("signing");
      const txs = await listForResale(resaleModalItem.content_id, resalePrice.trim());
      if (txs.length > 0) {
        if (!walletProvider) { openModal(); throw new Error("Wallet not connected"); }
        await signAndSendTransactions(walletProvider, txs, setResaleStatus);
      }
      setResaleStep("confirming");
      setResaleStatus(null);
      await confirmListForResale(resaleModalItem.content_id, resalePrice.trim());
      setResaleModalItem(null);
      setResalePrice("");
      setResaleStatus(null);
      setResaleStep("idle");
      fetchPurchased();
    } catch (e) { setResaleError(String(e)); setResaleStatus(null); setResaleStep("idle"); }
  };

  const handleCancelListing = async (item: LibraryItem) => {
    setCancellingId(item.content_id);
    try {
      const txs = await cancelResaleListing(item.content_id);
      if (txs.length > 0) {
        if (!walletProvider) { openModal(); throw new Error("Wallet not connected"); }
        await signAndSendTransactions(walletProvider, txs);
      }
      await confirmCancelListing(item.content_id);
      fetchPurchased();
    } catch (e) { setResaleError(String(e)); }
    finally { setCancellingId(null); }
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
        <button onClick={() => setActiveTab("collections")} className={tabCls("collections")}>
          Collections
          {collections.length > 0 && <span className="ml-2 badge-gray">{collections.length}</span>}
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
                    {activeListings[item.content_id] ? (
                      <>
                        <span className="text-xs px-2.5 py-1 rounded-full font-medium bg-amber-100 dark:bg-amber-900/20 text-amber-700 dark:text-amber-400">
                          Listed {activeListings[item.content_id].price_eth} ETH
                        </span>
                        <button onClick={() => handleCancelListing(item)}
                          disabled={cancellingId === item.content_id}
                          className="btn-danger text-xs px-2.5 py-1.5">
                          {cancellingId === item.content_id ? "…" : "Cancel"}
                        </button>
                      </>
                    ) : (
                      <button onClick={() => { setResaleModalItem(item); setResalePrice(""); setResaleError(null); }}
                        className="btn-secondary text-xs px-3 py-1.5">
                        List for Resale
                      </button>
                    )}
                  </div>
                </div>
              ))}
            </div>
          )}
        </>
      )}

      {/* Resale Price Modal */}
      {resaleModalItem && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50" onClick={() => { setResaleModalItem(null); setResaleStep("idle"); setResaleStatus(null); }}>
          <div className="bg-white dark:bg-slate-900 rounded-2xl shadow-xl p-6 w-full max-w-sm mx-4" onClick={(e) => e.stopPropagation()}>
            <h3 className="text-lg font-semibold text-slate-900 dark:text-slate-100 mb-1">
              List for Resale
            </h3>
            <p className="text-sm text-slate-500 dark:text-slate-500 mb-4 truncate">
              {resaleModalItem.title || "Untitled"}
            </p>
            <label className="label">Price (ETH)</label>
            <input type="text" value={resalePrice}
              onChange={(e) => setResalePrice(e.target.value)}
              disabled={resaleStep !== "idle"}
              placeholder="0.01" className="input-base mb-4" autoFocus />
            {resaleError && <div className="alert-error mb-3 text-xs">{resaleError}</div>}
            <div className="flex gap-3">
              <button onClick={handleListForResale}
                disabled={!resalePrice.trim() || resaleStep !== "idle"}
                className="btn-primary flex-1">
                {resaleStep === "signing" ? "Sign in wallet…"
                  : resaleStep === "confirming" ? "Confirming…"
                  : "List for Sale"}
              </button>
              <button onClick={() => { setResaleModalItem(null); setResaleStep("idle"); setResaleStatus(null); }}
                disabled={resaleStep !== "idle"}
                className="btn-ghost">
                Cancel
              </button>
            </div>
            {resaleStatus && resaleStep === "signing" && (
              <p className="text-xs text-slate-500 dark:text-slate-400 mt-3 text-center">{resaleStatus}</p>
            )}
          </div>
        </div>
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

      {/* ── Collections Tab ── */}
      {activeTab === "collections" && (
        <>
          <div className="flex justify-between items-center mb-4">
            <p className="text-sm text-slate-500 dark:text-slate-400">
              Manage your on-chain collections.
            </p>
            <button
              onClick={() => setShowCreateModal(true)}
              className="btn-primary text-sm"
            >
              Create Collection
            </button>
          </div>
          {loadingCollections ? (
            <div className="flex justify-center py-16">
              <div className="w-8 h-8 border-2 border-ara-500 border-t-transparent rounded-full animate-spin" />
            </div>
          ) : collections.length === 0 ? (
            <div className="card p-10 text-center">
              <p className="text-slate-500 dark:text-slate-400 mb-2">No collections yet.</p>
              <p className="text-xs text-slate-400 dark:text-slate-600">
                Create a collection to group your published content.
              </p>
            </div>
          ) : (
            <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
              {collections.map((c) => (
                <Link
                  key={c.collection_id}
                  to={`/collections/${c.collection_id}`}
                  className="card card-hover overflow-hidden"
                >
                  <div className="h-24 bg-gradient-to-br from-ara-600/30 to-purple-600/30 relative overflow-hidden">
                    {c.banner_uri && (
                      <img src={c.banner_uri} alt="" className="w-full h-full object-cover"
                        onError={(e) => { (e.target as HTMLImageElement).style.display = "none"; }} />
                    )}
                  </div>
                  <div className="p-3">
                    <h3 className="font-semibold text-sm truncate dark:text-white">{c.name}</h3>
                    <div className="flex items-center justify-between mt-1 text-xs text-gray-500 dark:text-gray-400">
                      <span>{c.item_count} items</span>
                      {parseFloat(c.volume_eth) > 0 && <span>{c.volume_eth} ETH vol</span>}
                    </div>
                  </div>
                </Link>
              ))}
            </div>
          )}

          {/* Create Collection Modal */}
          {showCreateModal && (
            <div className="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50 p-4"
              onClick={() => setShowCreateModal(false)}>
              <div className="card w-full max-w-md p-6 shadow-2xl" onClick={(e) => e.stopPropagation()}>
                <h3 className="font-semibold text-slate-900 dark:text-slate-100 mb-4">Create Collection</h3>
                {createError && <div className="alert-error text-xs mb-3">{createError}</div>}
                <div className="space-y-3 mb-5">
                  <div>
                    <label className="label">Name</label>
                    <input className="input-base w-full" value={newCollName}
                      onChange={(e) => setNewCollName(e.target.value)} placeholder="My Collection" />
                  </div>
                  <div>
                    <label className="label">Description</label>
                    <textarea className="input-base w-full" rows={3} value={newCollDesc}
                      onChange={(e) => setNewCollDesc(e.target.value)} placeholder="Optional description" />
                  </div>
                  <div>
                    <label className="label">Banner Image URL</label>
                    <input className="input-base w-full" value={newCollBanner}
                      onChange={(e) => setNewCollBanner(e.target.value)} placeholder="https://..." />
                  </div>
                </div>
                <div className="flex justify-end gap-3">
                  <button onClick={() => setShowCreateModal(false)} className="btn-ghost">Cancel</button>
                  <button
                    onClick={handleCreateCollection}
                    disabled={createStep !== "idle" || !newCollName.trim()}
                    className="btn-primary"
                  >
                    {createStep === "signing" ? "Sign in wallet..." : createStep === "confirming" ? "Confirming..." : "Create"}
                  </button>
                </div>
              </div>
            </div>
          )}
        </>
      )}
    </div>
  );
}

export default Library;
