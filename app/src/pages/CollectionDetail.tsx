import { useState, useEffect, useCallback, useMemo } from "react";
import { useParams, Link } from "react-router-dom";
import { useWeb3ModalAccount, useWeb3ModalProvider } from "@web3modal/ethers/react";
import {
  getCollection,
  getCollectionItems,
  getContentDetail,
  updateCollection,
  confirmUpdateCollection,
  deleteCollection,
  confirmDeleteCollection,
  removeFromCollection,
  confirmRemoveFromCollection,
  getCollectionAnalytics,
  getCollectionActivity,
} from "../lib/tauri";
import type {
  CollectionInfo,
  ContentDetail as ContentDetailType,
  CollectionAnalytics,
  CollectionActivity,
  PricePoint,
} from "../lib/tauri";
import { signAndSendTransactions } from "../lib/transactions";
import AddressDisplay from "../components/AddressDisplay";
import TabPanel from "../components/TabPanel";
import PriceHistoryChart from "../components/PriceHistoryChart";
import { convertFileSrc } from "@tauri-apps/api/core";
import { getPreviewAsset } from "../lib/tauri";

type Step = "idle" | "preparing" | "signing" | "confirming" | "done";

const COLLECTION_TABS = [
  { id: "items", label: "Items" },
  { id: "activity", label: "Activity" },
  { id: "analytics", label: "Analytics" },
];

export default function CollectionDetailPage() {
  const { collectionId } = useParams<{ collectionId: string }>();
  const { address } = useWeb3ModalAccount();
  const { walletProvider } = useWeb3ModalProvider();

  const [info, setInfo] = useState<CollectionInfo | null>(null);
  const [items, setItems] = useState<ContentDetailType[]>([]);
  const [analytics, setAnalytics] = useState<CollectionAnalytics | null>(null);
  const [activity, setActivity] = useState<CollectionActivity[]>([]);
  const [previewSrcs, setPreviewSrcs] = useState<Record<string, string>>({});
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Edit state
  const [editing, setEditing] = useState(false);
  const [editName, setEditName] = useState("");
  const [editDesc, setEditDesc] = useState("");
  const [editBanner, setEditBanner] = useState("");
  const [step, setStep] = useState<Step>("idle");

  // Filter/sort state
  const [searchQuery, setSearchQuery] = useState("");
  const [sortBy, setSortBy] = useState<"recent" | "price_asc" | "price_desc">("recent");
  const [typeFilter, setTypeFilter] = useState<string>("all");

  const isCreator =
    info && address && info.creator.toLowerCase() === address.toLowerCase();

  const id = collectionId ? parseInt(collectionId) : 0;

  const fetchData = useCallback(async () => {
    if (!id) return;
    try {
      setLoading(true);
      const collInfo = await getCollection(id);
      setInfo(collInfo);
      setEditName(collInfo.name);
      setEditDesc(collInfo.description);
      setEditBanner(collInfo.banner_uri);

      const contentIds = await getCollectionItems(id);
      const details = await Promise.all(
        contentIds.map((cid) =>
          getContentDetail(cid).catch(() => null)
        )
      );
      setItems(details.filter(Boolean) as ContentDetailType[]);

      // Fetch analytics + activity in parallel
      getCollectionAnalytics(id).then(setAnalytics).catch(() => {});
      getCollectionActivity(id, 50).then(setActivity).catch(() => {});
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [id]);

  useEffect(() => {
    fetchData();
  }, [fetchData]);

  // Load preview images
  useEffect(() => {
    items.forEach((item) => {
      if (previewSrcs[item.content_id]) return;
      try {
        const meta = JSON.parse(item.metadata_uri);
        if (!meta.main_preview_image?.hash) return;
        getPreviewAsset({
          contentId: item.content_id,
          previewHash: meta.main_preview_image.hash,
          filename: meta.main_preview_image.filename,
        })
          .then((path) => convertFileSrc(path, "localasset"))
          .then((src) =>
            setPreviewSrcs((prev) => ({ ...prev, [item.content_id]: src }))
          )
          .catch(() => {});
      } catch {}
    });
  }, [items]);

  // Filtered + sorted items
  const filteredItems = useMemo(() => {
    let result = [...items];
    if (searchQuery) {
      const q = searchQuery.toLowerCase();
      result = result.filter(
        (i) =>
          i.title.toLowerCase().includes(q) ||
          i.content_type.toLowerCase().includes(q)
      );
    }
    if (typeFilter !== "all") {
      result = result.filter((i) => i.content_type === typeFilter);
    }
    if (sortBy === "price_asc") {
      result.sort((a, b) => parseFloat(a.price_eth) - parseFloat(b.price_eth));
    } else if (sortBy === "price_desc") {
      result.sort((a, b) => parseFloat(b.price_eth) - parseFloat(a.price_eth));
    }
    return result;
  }, [items, searchQuery, typeFilter, sortBy]);

  // Convert activity to PricePoint[] for chart
  const activityAsPricePoints: PricePoint[] = useMemo(
    () =>
      activity.map((a) => ({
        price_eth: a.price_eth,
        block_number: a.block_number,
        buyer: a.buyer,
        tx_hash: a.tx_hash,
        is_resale: a.is_resale,
      })),
    [activity]
  );

  // Get unique content types for filter dropdown
  const contentTypes = useMemo(() => {
    const types = new Set(items.map((i) => i.content_type));
    return Array.from(types).sort();
  }, [items]);

  const handleUpdate = async () => {
    if (!walletProvider || !info) return;
    setError(null);
    try {
      setStep("preparing");
      const txs = await updateCollection({
        collectionId: id,
        name: editName,
        description: editDesc,
        bannerUri: editBanner,
      });
      setStep("signing");
      await signAndSendTransactions(walletProvider, txs);
      setStep("confirming");
      await confirmUpdateCollection({
        collectionId: id,
        name: editName,
        description: editDesc,
        bannerUri: editBanner,
      });
      setStep("done");
      setEditing(false);
      fetchData();
      setTimeout(() => setStep("idle"), 1500);
    } catch (e) {
      setError(String(e));
      setStep("idle");
    }
  };

  const handleDelete = async () => {
    if (!walletProvider || !info) return;
    if (!confirm("Delete this collection? Items will be unlinked.")) return;
    setError(null);
    try {
      setStep("preparing");
      const txs = await deleteCollection(id);
      setStep("signing");
      await signAndSendTransactions(walletProvider, txs);
      setStep("confirming");
      await confirmDeleteCollection(id);
      setStep("done");
      window.history.back();
    } catch (e) {
      setError(String(e));
      setStep("idle");
    }
  };

  const handleRemoveItem = async (contentId: string) => {
    if (!walletProvider) return;
    if (!confirm("Remove this item from the collection?")) return;
    try {
      const txs = await removeFromCollection(id, contentId);
      await signAndSendTransactions(walletProvider, txs);
      await confirmRemoveFromCollection(id, contentId);
      fetchData();
    } catch (e) {
      setError(String(e));
    }
  };

  if (loading) {
    return (
      <div className="p-6">
        <div className="animate-pulse space-y-4">
          <div className="h-48 bg-gray-200 dark:bg-gray-800 rounded-xl" />
          <div className="h-6 w-48 bg-gray-200 dark:bg-gray-800 rounded" />
        </div>
      </div>
    );
  }

  if (!info) {
    return (
      <div className="p-6">
        <div className="alert-error">Collection not found</div>
      </div>
    );
  }

  return (
    <div className="max-w-6xl mx-auto space-y-6">
      {/* Banner */}
      <div className="h-48 rounded-xl bg-gradient-to-br from-ara-600/30 to-purple-600/30 relative overflow-hidden">
        {info.banner_uri && (
          <img
            src={info.banner_uri}
            alt=""
            className="w-full h-full object-cover"
            onError={(e) => {
              (e.target as HTMLImageElement).style.display = "none";
            }}
          />
        )}
        <div className="absolute inset-0 bg-gradient-to-t from-black/50 to-transparent" />
        <div className="absolute bottom-4 left-6">
          <h1 className="text-2xl font-bold text-white">{info.name}</h1>
          <div className="text-white/70 text-sm mt-1">
            by <AddressDisplay address={info.creator} className="text-white/90" />
          </div>
        </div>
      </div>

      {/* Stats bar */}
      <div className="flex flex-wrap gap-8 px-1">
        <div className="text-center">
          <div className="text-xl font-bold dark:text-white">{info.item_count}</div>
          <div className="text-xs text-gray-500">Items</div>
        </div>
        <div className="text-center">
          <div className="text-xl font-bold dark:text-white">
            {analytics
              ? parseFloat(analytics.floor_price_eth) > 0
                ? `${analytics.floor_price_eth} ETH`
                : "---"
              : "---"}
          </div>
          <div className="text-xs text-gray-500">Floor Price</div>
        </div>
        <div className="text-center">
          <div className="text-xl font-bold dark:text-white">
            {info.volume_eth} ETH
          </div>
          <div className="text-xs text-gray-500">Total Volume</div>
        </div>
        <div className="text-center">
          <div className="text-xl font-bold dark:text-white">
            {analytics?.unique_owners ?? "---"}
          </div>
          <div className="text-xs text-gray-500">Owners</div>
        </div>
        <div className="text-center">
          <div className="text-xl font-bold dark:text-white">
            {analytics?.total_sales ?? 0}
          </div>
          <div className="text-xs text-gray-500">Sales</div>
        </div>
      </div>

      {/* Description */}
      {info.description && (
        <p className="text-gray-600 dark:text-gray-400 text-sm">
          {info.description}
        </p>
      )}

      {error && <div className="alert-error">{error}</div>}

      {/* Creator controls */}
      {isCreator && (
        <div className="flex gap-2">
          <button
            onClick={() => setEditing(!editing)}
            className="btn-secondary text-sm"
          >
            {editing ? "Cancel Edit" : "Edit Collection"}
          </button>
          <button onClick={handleDelete} className="btn-danger text-sm">
            Delete
          </button>
        </div>
      )}

      {/* Edit form */}
      {editing && (
        <div className="card p-4 space-y-3">
          <div>
            <label className="label">Name</label>
            <input
              className="input-base w-full"
              value={editName}
              onChange={(e) => setEditName(e.target.value)}
            />
          </div>
          <div>
            <label className="label">Description</label>
            <textarea
              className="input-base w-full"
              rows={3}
              value={editDesc}
              onChange={(e) => setEditDesc(e.target.value)}
            />
          </div>
          <div>
            <label className="label">Banner URL</label>
            <input
              className="input-base w-full"
              value={editBanner}
              onChange={(e) => setEditBanner(e.target.value)}
            />
          </div>
          <button
            onClick={handleUpdate}
            disabled={step !== "idle"}
            className="btn-primary text-sm"
          >
            {step === "signing"
              ? "Sign in wallet..."
              : step === "confirming"
                ? "Confirming..."
                : step === "done"
                  ? "Updated!"
                  : "Save Changes"}
          </button>
        </div>
      )}

      {/* Tabbed content */}
      <TabPanel tabs={COLLECTION_TABS} defaultTab="items">
        {(activeTab) => {
          if (activeTab === "items") {
            return (
              <div>
                {/* Search + Sort + Filter */}
                <div className="flex flex-wrap items-center gap-3 mb-4">
                  <input
                    type="text"
                    placeholder="Search items..."
                    value={searchQuery}
                    onChange={(e) => setSearchQuery(e.target.value)}
                    className="input-base text-sm py-1.5 px-3 w-48"
                  />
                  <select
                    value={sortBy}
                    onChange={(e) => setSortBy(e.target.value as typeof sortBy)}
                    className="input-base text-sm py-1.5 px-3 w-auto"
                  >
                    <option value="recent">Most Recent</option>
                    <option value="price_asc">Price: Low to High</option>
                    <option value="price_desc">Price: High to Low</option>
                  </select>
                  {contentTypes.length > 1 && (
                    <select
                      value={typeFilter}
                      onChange={(e) => setTypeFilter(e.target.value)}
                      className="input-base text-sm py-1.5 px-3 w-auto"
                    >
                      <option value="all">All Types</option>
                      {contentTypes.map((t) => (
                        <option key={t} value={t}>
                          {t.charAt(0).toUpperCase() + t.slice(1)}
                        </option>
                      ))}
                    </select>
                  )}
                  <span className="text-xs text-gray-400 ml-auto">
                    {filteredItems.length} item{filteredItems.length !== 1 ? "s" : ""}
                  </span>
                </div>

                {/* Items grid */}
                {filteredItems.length === 0 ? (
                  <div className="text-gray-400 text-sm py-8 text-center">
                    {items.length === 0
                      ? "No items in this collection yet."
                      : "No items match your filters."}
                  </div>
                ) : (
                  <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
                    {filteredItems.map((item) => {
                      const isSoldOut =
                        item.max_supply > 0 && item.total_minted >= item.max_supply;
                      return (
                        <div key={item.content_id} className="relative group">
                          <Link
                            to={`/content/${encodeURIComponent(item.content_id)}`}
                            className="card card-hover overflow-hidden block"
                          >
                            <div className="aspect-[4/3] bg-gradient-to-br from-gray-100 to-gray-200 dark:from-gray-800 dark:to-gray-900 relative overflow-hidden">
                              {previewSrcs[item.content_id] ? (
                                <img
                                  src={previewSrcs[item.content_id]}
                                  alt=""
                                  className="w-full h-full object-cover"
                                />
                              ) : (
                                <div className="flex items-center justify-center h-full text-gray-400 text-3xl">
                                  {item.content_type === "music"
                                    ? "\u266B"
                                    : item.content_type === "video"
                                      ? "\u25B6"
                                      : item.content_type === "game"
                                        ? "\u{1F3AE}"
                                        : "\u{1F4C4}"}
                                </div>
                              )}
                              <span className="absolute top-2 left-2 px-2 py-0.5 bg-black/50 backdrop-blur-sm text-white text-[10px] font-semibold uppercase tracking-wider rounded-full">
                                {item.content_type}
                              </span>
                              {isSoldOut && (
                                <span className="absolute top-2 right-2 bg-red-500 text-white text-[10px] px-2 py-0.5 rounded-full">
                                  Sold Out
                                </span>
                              )}
                            </div>
                            <div className="p-3">
                              <h3 className="font-medium text-sm truncate dark:text-white">
                                {item.title}
                              </h3>
                              <div className="text-xs text-gray-500 mt-1">
                                {item.price_eth} ETH
                              </div>
                            </div>
                          </Link>
                          {isCreator && (
                            <button
                              onClick={() => handleRemoveItem(item.content_id)}
                              className="absolute top-2 left-2 bg-red-500/80 text-white rounded-full w-6 h-6 flex items-center justify-center text-xs opacity-0 group-hover:opacity-100 transition-opacity"
                              title="Remove from collection"
                            >
                              x
                            </button>
                          )}
                        </div>
                      );
                    })}
                  </div>
                )}
              </div>
            );
          }

          if (activeTab === "activity") {
            if (activity.length === 0) {
              return (
                <div className="text-gray-400 dark:text-gray-500 text-sm text-center py-8">
                  No activity yet
                </div>
              );
            }
            return (
              <div className="overflow-x-auto">
                <table className="w-full text-sm">
                  <thead>
                    <tr className="text-left text-gray-500 dark:text-gray-400 border-b border-gray-200 dark:border-gray-700">
                      <th className="pb-2 font-medium">Event</th>
                      <th className="pb-2 font-medium">Item</th>
                      <th className="pb-2 font-medium">Price</th>
                      <th className="pb-2 font-medium">Buyer</th>
                      <th className="pb-2 font-medium">Tx</th>
                    </tr>
                  </thead>
                  <tbody>
                    {activity.map((a, i) => (
                      <tr
                        key={i}
                        className="border-b border-gray-100 dark:border-gray-800"
                      >
                        <td className="py-2">
                          <span
                            className={`text-xs px-2 py-0.5 rounded-full ${
                              a.is_resale
                                ? "bg-amber-100 text-amber-700 dark:bg-amber-900/30 dark:text-amber-400"
                                : "bg-purple-100 text-purple-700 dark:bg-purple-900/30 dark:text-purple-400"
                            }`}
                          >
                            {a.is_resale ? "Resale" : "Sale"}
                          </span>
                        </td>
                        <td className="py-2">
                          <Link
                            to={`/content/${encodeURIComponent(a.content_id)}`}
                            className="text-ara-500 hover:underline truncate max-w-[160px] inline-block"
                          >
                            {a.title}
                          </Link>
                        </td>
                        <td className="py-2 font-mono">{a.price_eth} ETH</td>
                        <td className="py-2">
                          <AddressDisplay address={a.buyer} />
                        </td>
                        <td className="py-2">
                          {a.tx_hash && (
                            <a
                              href={`https://sepolia.etherscan.io/tx/${a.tx_hash}`}
                              target="_blank"
                              rel="noopener noreferrer"
                              className="text-ara-500 hover:underline font-mono text-xs"
                            >
                              {a.tx_hash.slice(0, 8)}...
                            </a>
                          )}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            );
          }

          if (activeTab === "analytics") {
            return (
              <div className="space-y-6">
                {/* Stats cards */}
                <div className="grid grid-cols-2 lg:grid-cols-4 gap-4">
                  <div className="card p-4 text-center">
                    <div className="text-2xl font-bold dark:text-white">
                      {analytics?.total_sales ?? 0}
                    </div>
                    <div className="text-xs text-gray-500">Total Sales</div>
                  </div>
                  <div className="card p-4 text-center">
                    <div className="text-2xl font-bold dark:text-white">
                      {analytics?.total_minted ?? 0}
                    </div>
                    <div className="text-xs text-gray-500">Total Minted</div>
                  </div>
                  <div className="card p-4 text-center">
                    <div className="text-2xl font-bold dark:text-white">
                      {analytics?.unique_owners ?? 0}
                    </div>
                    <div className="text-xs text-gray-500">Unique Owners</div>
                  </div>
                  <div className="card p-4 text-center">
                    <div className="text-2xl font-bold dark:text-white">
                      {info.volume_eth} ETH
                    </div>
                    <div className="text-xs text-gray-500">Volume</div>
                  </div>
                </div>

                {/* Price history chart */}
                <div>
                  <h3 className="text-sm font-semibold dark:text-white mb-3">
                    Sales History
                  </h3>
                  <PriceHistoryChart data={activityAsPricePoints} />
                </div>
              </div>
            );
          }

          return null;
        }}
      </TabPanel>
    </div>
  );
}
