import { useState, useEffect, useRef } from "react";
import { useParams, Link } from "react-router-dom";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  getContentDetail,
  purchaseContent,
  confirmPurchase,
  updateContent,
  confirmUpdateContent,
  broadcastDeliveryReceipt,
  getMarketplaceAddress,
  getPreviewAsset,
  openDownloadedContent,
  openContentFolder,
  getEditionInfo,
  getResaleListings,
  buyResale,
  getPriceHistory,
  getItemAnalytics,
  getContentCollection,
  getCollection,
  type ContentDetail as ContentDetailType,
  type ContentMetadataV2,
  type EditionInfo,
  type ResaleListing,
  type PricePoint,
  type ItemAnalytics,
  type CollectionInfo,
} from "../lib/tauri";
import { signAndSendTransactions } from "../lib/transactions";
import {
  useWeb3Modal,
  useWeb3ModalAccount,
  useWeb3ModalProvider,
} from "@web3modal/ethers/react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { CATEGORIES_BY_TYPE } from "../lib/categories";
import type { ContentType } from "../lib/types";
import { fmtBytes } from "../lib/format";
import AddressDisplay from "../components/AddressDisplay";
import { IconShare } from "../components/Icons";
import TabPanel from "../components/TabPanel";
import PriceHistoryChart from "../components/PriceHistoryChart";
import ActivityTable from "../components/ActivityTable";
import TraitsGrid from "../components/TraitsGrid";

type PurchaseStep = "idle" | "preparing" | "signing" | "confirming" | "done";
type EditStep = "idle" | "preparing" | "signing" | "confirming" | "done";

interface CarouselItem {
  hash: string;
  filename: string;
  type: "image" | "video";
  src: string | null;
  loading: boolean;
}

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

const getCategories = (type?: string) =>
  CATEGORIES_BY_TYPE[(type ?? "other") as ContentType] ?? CATEGORIES_BY_TYPE.other;


const DETAIL_TABS = [
  { id: "details", label: "Details" },
  { id: "properties", label: "Properties" },
  { id: "activity", label: "Activity" },
  { id: "listings", label: "Listings" },
];

function ContentDetail() {
  const { contentId } = useParams<{ contentId: string }>();
  const { open: openModal } = useWeb3Modal();
  const { isConnected, address } = useWeb3ModalAccount();
  const { walletProvider } = useWeb3ModalProvider();

  const [content, setContent] = useState<ContentDetailType | null>(null);
  const [meta, setMeta] = useState<ContentMetadataV2 | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [purchaseStep, setPurchaseStep] = useState<PurchaseStep>("idle");
  const [purchaseError, setPurchaseError] = useState<string | null>(null);
  const [purchaseTxHash, setPurchaseTxHash] = useState<string | null>(null);
  const [receiptStep, setReceiptStep] = useState<"idle" | "signing" | "done" | "skipped">("idle");
  const [downloadPath, setDownloadPath] = useState<string | null>(null);
  const [downloadProgress, setDownloadProgress] = useState<{ received: number; total: number } | null>(null);
  const unlistenProgressRef = useRef<UnlistenFn | null>(null);

  // Preview carousel state
  const [carouselItems, setCarouselItems] = useState<CarouselItem[]>([]);
  const [carouselIndex, setCarouselIndex] = useState(0);

  // Edition + resale state
  const [edition, setEdition] = useState<EditionInfo | null>(null);
  const [resaleListings, setResaleListings] = useState<ResaleListing[]>([]);
  const [buyResaleStep, setBuyResaleStep] = useState<"idle" | "preparing" | "signing" | "confirming" | "done">("idle");
  const [buyResaleError, setBuyResaleError] = useState<string | null>(null);
  const [buyResaleSeller, setBuyResaleSeller] = useState<string | null>(null);

  // Analytics state
  const [priceHistory, setPriceHistory] = useState<PricePoint[]>([]);
  const [analytics, setAnalytics] = useState<ItemAnalytics | null>(null);

  // Collection state
  const [collection, setCollection] = useState<CollectionInfo | null>(null);

  // Edit state
  const [editing, setEditing] = useState(false);
  const [editTitle, setEditTitle] = useState("");
  const [editDescription, setEditDescription] = useState("");
  const [editContentType, setEditContentType] = useState("");
  const [editPriceEth, setEditPriceEth] = useState("");
  const [editCategories, setEditCategories] = useState<string[]>([]);
  const [editStep, setEditStep] = useState<EditStep>("idle");
  const [editError, setEditError] = useState<string | null>(null);
  const [linkCopied, setLinkCopied] = useState(false);

  const loadContent = async (id: string) => {
    setLoading(true);
    try {
      const c = await getContentDetail(id);
      setContent(c);
      try {
        const parsed: ContentMetadataV2 = JSON.parse(c.metadata_uri);
        setMeta(parsed);
      } catch {
        setMeta(null);
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    if (!contentId) return;
    loadContent(decodeURIComponent(contentId));
  }, [contentId]);

  // Clean up progress event listener on unmount
  useEffect(() => {
    return () => {
      if (unlistenProgressRef.current) {
        unlistenProgressRef.current();
        unlistenProgressRef.current = null;
      }
    };
  }, []);

  // Load edition info + resale listings
  useEffect(() => {
    if (!content) return;
    const id = content.content_id;
    getEditionInfo(id).then(setEdition).catch(() => setEdition(null));
    getResaleListings(id).then(setResaleListings).catch(() => setResaleListings([]));
  }, [content]);

  // Load analytics data
  useEffect(() => {
    if (!content) return;
    const id = content.content_id;
    getPriceHistory(id).then(setPriceHistory).catch(() => setPriceHistory([]));
    getItemAnalytics(id).then(setAnalytics).catch(() => setAnalytics(null));
  }, [content]);

  // Load collection info
  useEffect(() => {
    if (!content) return;
    getContentCollection(content.content_id)
      .then((collId) => {
        if (collId && collId > 0) {
          return getCollection(collId).then(setCollection);
        }
        setCollection(null);
      })
      .catch(() => setCollection(null));
  }, [content]);

  // Load preview assets lazily after content + meta are set
  useEffect(() => {
    if (!content || !meta) return;
    const id = content.content_id;

    const items: CarouselItem[] = [
      ...(meta.main_preview_image
        ? [{ hash: meta.main_preview_image.hash, filename: meta.main_preview_image.filename, type: "image" as const, src: null, loading: true }]
        : []),
      ...(meta.main_preview_trailer
        ? [{ hash: meta.main_preview_trailer.hash, filename: meta.main_preview_trailer.filename, type: "video" as const, src: null, loading: true }]
        : []),
      ...(meta.previews ?? []).map((p) => ({
        hash: p.hash,
        filename: p.filename,
        type: p.type as "image" | "video",
        src: null as string | null,
        loading: true,
      })),
    ];

    setCarouselItems(items);
    setCarouselIndex(0);

    items.forEach((item, i) => {
      getPreviewAsset({ contentId: id, previewHash: item.hash, filename: item.filename })
        .then((localPath) => convertFileSrc(localPath, "localasset"))
        .then((src) =>
          setCarouselItems((prev) =>
            prev.map((x, idx) => (idx === i ? { ...x, src, loading: false } : x))
          )
        )
        .catch(() =>
          setCarouselItems((prev) =>
            prev.map((x, idx) => (idx === i ? { ...x, loading: false } : x))
          )
        );
    });
  }, [content, meta]);

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
    setEditCategories(content.categories ?? []);
    setEditError(null);
    setEditStep("idle");
    setEditing(true);
  };

  const cancelEditing = () => {
    setEditing(false);
    setEditError(null);
    setEditStep("idle");
  };

  const toggleEditCategory = (cat: string) => {
    setEditCategories((prev) =>
      prev.includes(cat) ? prev.filter((c) => c !== cat) : [...prev, cat]
    );
  };

  const handleUpdate = async () => {
    if (!contentId || !isConnected) return;
    setEditError(null);
    try {
      const decodedId = decodeURIComponent(contentId);
      setEditStep("preparing");
      const transactions = await updateContent({
        contentId: decodedId,
        title: editTitle.trim(),
        description: editDescription.trim(),
        contentType: editContentType,
        priceEth: editPriceEth.trim(),
        categories: editCategories,
      });

      if (!walletProvider) {
        openModal();
        throw new Error(
          "Wallet session expired — reconnect your wallet in the dialog that just opened, then try again."
        );
      }
      setEditStep("signing");
      await signAndSendTransactions(walletProvider, transactions);

      setEditStep("confirming");
      await confirmUpdateContent({
        contentId: decodedId,
        title: editTitle.trim(),
        description: editDescription.trim(),
        contentType: editContentType,
        priceEth: editPriceEth.trim(),
        categories: editCategories,
      });

      setEditStep("done");
      await loadContent(decodedId);
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
    if (!contentId) return;
    if (!isConnected) { openModal(); return; }
    setPurchaseError(null);
    setDownloadProgress(null);
    try {
      setPurchaseStep("preparing");
      const result = await purchaseContent(decodeURIComponent(contentId));

      let txHash = "0x0";
      if (result.transactions.length > 0) {
        if (!walletProvider) {
          openModal();
          throw new Error(
            "Wallet session expired — reconnect your wallet in the dialog that just opened, then try again."
          );
        }
        setPurchaseStep("signing");
        txHash = await signAndSendTransactions(walletProvider, result.transactions);
      }

      // Start listening for download progress before confirming (which triggers download)
      const unlisten = await listen<{ content_id: string; bytes_received: number; total_bytes: number }>(
        "download-progress",
        (event) => {
          setDownloadProgress({ received: event.payload.bytes_received, total: event.payload.total_bytes });
        }
      );
      unlistenProgressRef.current = unlisten;

      setPurchaseStep("confirming");
      const confirmResult = await confirmPurchase(result.content_id, txHash);
      setDownloadPath(confirmResult.download_path);

      // Clean up progress listener
      unlisten();
      unlistenProgressRef.current = null;

      setPurchaseTxHash(txHash !== "0x0" ? txHash : null);
      setPurchaseStep("done");

      // Refresh edition info so "0/1 minted" updates to "1/1 minted" immediately
      const decodedId = decodeURIComponent(contentId);
      getEditionInfo(decodedId).then(setEdition).catch(() => {});

      // Auto-sign delivery receipt (gasless signature, no ETH cost).
      handleSignReceipt();
    } catch (err) {
      if (unlistenProgressRef.current) {
        unlistenProgressRef.current();
        unlistenProgressRef.current = null;
      }
      setPurchaseError(String(err));
      setPurchaseStep("idle");
      setDownloadProgress(null);
    }
  };

  const handleSignReceipt = async () => {
    if (!content || !address || !walletProvider) return;
    setReceiptStep("signing");
    try {
      const marketplaceAddr = await getMarketplaceAddress();
      if (!marketplaceAddr) throw new Error("Marketplace not configured");
      const timestamp = Math.floor(Date.now() / 1000);
      const bytesServed = meta?.file_size ?? 0;
      const typedData = {
        types: {
          EIP712Domain: [
            { name: "name", type: "string" },
            { name: "version", type: "string" },
            { name: "chainId", type: "uint256" },
            { name: "verifyingContract", type: "address" },
          ],
          DeliveryReceipt: [
            { name: "contentId", type: "bytes32" },
            { name: "seederEthAddress", type: "address" },
            { name: "bytesServed", type: "uint256" },
            { name: "timestamp", type: "uint256" },
          ],
        },
        primaryType: "DeliveryReceipt",
        domain: {
          name: "AraMarketplace",
          version: "1",
          chainId: 11155111,
          verifyingContract: marketplaceAddr,
        },
        message: {
          contentId: content.content_id,
          seederEthAddress: content.creator,
          bytesServed: String(bytesServed),
          timestamp: String(timestamp),
        },
      };
      const signature = await (walletProvider as {
        request: (args: { method: string; params: unknown[] }) => Promise<string>;
      }).request({
        method: "eth_signTypedData_v4",
        params: [address, JSON.stringify(typedData)],
      });
      await broadcastDeliveryReceipt({
        contentId: content.content_id,
        seederEthAddress: content.creator,
        buyerEthAddress: address,
        signature,
        timestamp,
        bytesServed,
      });
      setReceiptStep("done");
    } catch {
      setReceiptStep("skipped");
    }
  };

  const isSoldOut = edition
    ? edition.max_supply > 0 && edition.total_minted >= edition.max_supply
    : false;

  const handleBuyResale = async (seller: string) => {
    if (!contentId || !isConnected) return;
    setBuyResaleError(null);
    setBuyResaleSeller(seller);
    try {
      setBuyResaleStep("preparing");
      const result = await buyResale(decodeURIComponent(contentId), seller);
      if (!walletProvider) {
        openModal();
        throw new Error("Wallet session expired — reconnect then try again.");
      }
      setBuyResaleStep("signing");
      const txHash = await signAndSendTransactions(walletProvider, result.transactions);
      setBuyResaleStep("confirming");
      await confirmPurchase(result.content_id, txHash);
      setBuyResaleStep("done");
      getResaleListings(decodeURIComponent(contentId)).then(setResaleListings).catch(() => {});
      getEditionInfo(decodeURIComponent(contentId)).then(setEdition).catch(() => {});
      setTimeout(() => { setBuyResaleStep("idle"); setBuyResaleSeller(null); }, 2000);
    } catch (err) {
      setBuyResaleError(String(err));
      setBuyResaleStep("idle");
      setBuyResaleSeller(null);
    }
  };

  const contentTypeIcon = (type: string) => {
    switch (type) {
      case "game": return "🎮";
      case "music": return "🎵";
      case "video": return "🎬";
      case "document": return "📄";
      case "software": return "💻";
      default: return "📦";
    }
  };

  // Build traits for properties tab
  const buildTraits = () => {
    if (!content) return [];
    const traits: { label: string; value: string }[] = [];
    traits.push({ label: "Content Type", value: content.content_type || "other" });
    if (content.categories && content.categories.length > 0) {
      traits.push({ label: "Categories", value: content.categories.join(", ") });
    }
    if (edition) {
      traits.push({
        label: "Edition",
        value: edition.max_supply === 0
          ? "Unlimited"
          : `${edition.total_minted}/${edition.max_supply} minted`,
      });
      if (edition.royalty_bps > 0) {
        traits.push({
          label: "Creator Royalty",
          value: `${(edition.royalty_bps / 100).toFixed(edition.royalty_bps % 100 === 0 ? 0 : 1)}%`,
        });
      }
    }
    if (meta?.file_size && meta.file_size > 0) {
      traits.push({ label: "File Size", value: fmtBytes(meta.file_size) });
    }
    if (analytics) {
      traits.push({ label: "Total Sales", value: String(analytics.total_sales) });
      traits.push({ label: "Total Volume", value: `${analytics.total_volume_eth} ETH` });
      traits.push({ label: "Unique Buyers", value: String(analytics.unique_buyers) });
    }
    return traits;
  };

  if (loading) {
    return (
      <div className="flex items-center justify-center py-20">
        <div className="text-center space-y-3">
          <div className="w-8 h-8 border-2 border-ara-500 border-t-transparent rounded-full animate-spin mx-auto" />
          <p className="text-sm text-slate-400 dark:text-slate-600">Loading content...</p>
        </div>
      </div>
    );
  }

  if (error || !content) {
    return (
      <div className="max-w-2xl">
        <Link
          to="/"
          className="inline-flex items-center gap-1.5 text-sm text-ara-600 dark:text-ara-400 hover:text-ara-500 dark:hover:text-ara-300 mb-4"
        >
          &larr; Back to Marketplace
        </Link>
        <div className="alert-error">{error || "Content not found"}</div>
      </div>
    );
  }

  const canPurchase = purchaseStep === "idle" && content.active && !isCreator;

  return (
    <div className="max-w-2xl">
      <Link
        to="/"
        className="inline-flex items-center gap-1.5 text-sm text-ara-600 dark:text-ara-400 hover:text-ara-500 dark:hover:text-ara-300 mb-4"
      >
        &larr; Back to Marketplace
      </Link>

      <div className="card mt-2 overflow-hidden">
        {editing ? (
          /* ---- Edit Mode ---- */
          <div className="p-6 space-y-4">
            <h2 className="text-lg font-semibold text-slate-900 dark:text-slate-100">Edit Content</h2>

            <div>
              <label className="label">Title</label>
              <input
                type="text"
                value={editTitle}
                onChange={(e) => setEditTitle(e.target.value)}
                disabled={editStep !== "idle"}
                className="input-base"
              />
            </div>

            <div>
              <label className="label">Description</label>
              <textarea
                value={editDescription}
                onChange={(e) => setEditDescription(e.target.value)}
                disabled={editStep !== "idle"}
                rows={3}
                className="input-base resize-none"
              />
            </div>

            <div>
              <label className="label">Content Type</label>
              <select
                value={editContentType}
                onChange={(e) => setEditContentType(e.target.value)}
                disabled={editStep !== "idle"}
                className="input-base"
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
              <label className="label">
                Categories{" "}
                <span className="text-slate-400 dark:text-slate-500 font-normal">(select all that apply)</span>
              </label>
              <div className="flex flex-wrap gap-1.5">
                {getCategories(editContentType).map((cat) => {
                  const sel = editCategories.includes(cat);
                  return (
                    <button
                      key={cat}
                      type="button"
                      onClick={() => toggleEditCategory(cat)}
                      disabled={editStep !== "idle"}
                      className={`px-3 py-1 rounded-full text-xs font-medium border transition-colors disabled:opacity-50 ${
                        sel
                          ? "bg-ara-600 border-ara-600 text-white"
                          : "border-slate-300 dark:border-slate-700 text-slate-600 dark:text-slate-400 hover:border-ara-400 dark:hover:border-ara-600 bg-white dark:bg-slate-900"
                      }`}
                    >
                      {cat}
                    </button>
                  );
                })}
              </div>
            </div>

            <div>
              <label className="label">Price (ETH)</label>
              <input
                type="text"
                value={editPriceEth}
                onChange={(e) => setEditPriceEth(e.target.value)}
                disabled={editStep !== "idle"}
                className="input-base"
              />
            </div>

            {editError && <div className="alert-error">{editError}</div>}

            {editStep === "done" ? (
              <div className="alert-success font-medium">Content updated successfully!</div>
            ) : (
              <div className="flex gap-3">
                <button
                  onClick={handleUpdate}
                  disabled={editStep !== "idle" || !editTitle.trim() || !editPriceEth.trim()}
                  className="btn-primary flex-1"
                >
                  {EDIT_STEP_LABELS[editStep]}
                </button>
                <button
                  onClick={cancelEditing}
                  disabled={editStep !== "idle"}
                  className="btn-ghost"
                >
                  Cancel
                </button>
              </div>
            )}
          </div>
        ) : (
          /* ---- View Mode ---- */
          <>
            {/* 16:9 preview carousel */}
            {carouselItems.length > 0 && (() => {
              const current = carouselItems[carouselIndex];
              return (
                <div className="w-full bg-black select-none">
                  <div className="relative w-full aspect-video flex items-center justify-center">
                    {current.loading ? (
                      <div className="flex flex-col items-center gap-2">
                        <div className="w-6 h-6 border-2 border-ara-500 border-t-transparent rounded-full animate-spin" />
                        <span className="text-slate-500 text-xs">Loading preview...</span>
                      </div>
                    ) : current.src ? (
                      current.type === "video" ? (
                        <video
                          key={current.src}
                          src={current.src}
                          controls
                          className="w-full h-full object-contain"
                        />
                      ) : (
                        <img
                          src={current.src}
                          alt={current.filename}
                          className="w-full h-full object-contain"
                        />
                      )
                    ) : (
                      <div className="text-slate-600 text-sm">Preview unavailable</div>
                    )}

                    {carouselItems.length > 1 && (
                      <>
                        <button
                          onClick={() =>
                            setCarouselIndex((i) => (i - 1 + carouselItems.length) % carouselItems.length)
                          }
                          className="absolute left-2 top-1/2 -translate-y-1/2 bg-black/60 hover:bg-black/80 text-white w-8 h-8 rounded-full flex items-center justify-center transition-colors text-lg leading-none"
                          aria-label="Previous"
                        >
                          &#8249;
                        </button>
                        <button
                          onClick={() =>
                            setCarouselIndex((i) => (i + 1) % carouselItems.length)
                          }
                          className="absolute right-2 top-1/2 -translate-y-1/2 bg-black/60 hover:bg-black/80 text-white w-8 h-8 rounded-full flex items-center justify-center transition-colors text-lg leading-none"
                          aria-label="Next"
                        >
                          &#8250;
                        </button>
                      </>
                    )}
                  </div>

                  {carouselItems.length > 1 && (
                    <div className="flex gap-1.5 px-2 py-2 overflow-x-auto bg-black/80">
                      {carouselItems.map((item, i) => (
                        <button
                          key={i}
                          onClick={() => setCarouselIndex(i)}
                          className={`flex-shrink-0 rounded overflow-hidden border-2 transition-all focus:outline-none ${
                            i === carouselIndex
                              ? "border-ara-500"
                              : "border-transparent opacity-50 hover:opacity-80"
                          }`}
                          style={{ width: 80, height: 45 }}
                          aria-label={`Preview ${i + 1}`}
                        >
                          {item.src ? (
                            item.type === "video" ? (
                              <video src={item.src} className="w-full h-full object-cover" />
                            ) : (
                              <img src={item.src} alt={item.filename} className="w-full h-full object-cover" />
                            )
                          ) : (
                            <div className="w-full h-full bg-slate-800 flex items-center justify-center text-slate-500 text-xs">
                              {item.loading ? "..." : "x"}
                            </div>
                          )}
                        </button>
                      ))}
                    </div>
                  )}
                </div>
              );
            })()}

            <div className="p-6">
              {/* Title + badges + price */}
              <div className="flex items-start gap-4">
                {carouselItems.length === 0 && (
                  <div className="text-5xl">{contentTypeIcon(content.content_type)}</div>
                )}
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2 flex-wrap">
                    <h1 className="text-2xl font-bold text-slate-900 dark:text-slate-100">
                      {content.title || "Untitled"}
                    </h1>
                    {content.updated_at !== null && (
                      <span className="badge-blue">File updated</span>
                    )}
                    <button
                      onClick={() => {
                        const link = `ara://content/${encodeURIComponent(content.content_id)}`;
                        navigator.clipboard.writeText(link);
                        setLinkCopied(true);
                        setTimeout(() => setLinkCopied(false), 2000);
                      }}
                      className="inline-flex items-center gap-1.5 text-sm font-medium px-3 py-1.5 rounded-lg bg-indigo-50 dark:bg-indigo-900/30 text-indigo-600 dark:text-indigo-400 hover:bg-indigo-100 dark:hover:bg-indigo-900/50 border border-indigo-200 dark:border-indigo-800 transition-colors"
                      title="Copy shareable link"
                    >
                      <IconShare className="w-4 h-4" />
                      {linkCopied ? "Copied!" : "Share"}
                    </button>
                  </div>
                  <p className="text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-500 mt-1">
                    {content.content_type}
                  </p>

                  {/* Creator with friendly name */}
                  <div className="flex items-center gap-1.5 mt-1.5 text-xs text-slate-500 dark:text-slate-400">
                    <span>by</span>
                    <AddressDisplay address={content.creator} className="font-medium text-slate-700 dark:text-slate-300" />
                  </div>

                  {/* Collection link */}
                  {collection && (
                    <Link
                      to={`/collections/${collection.collection_id}`}
                      className="inline-flex items-center gap-1.5 mt-1.5 text-xs text-ara-600 dark:text-ara-400 hover:text-ara-500"
                    >
                      <span className="w-4 h-4 rounded bg-gradient-to-br from-ara-500/30 to-purple-500/30 inline-block flex-shrink-0" />
                      {collection.name}
                    </Link>
                  )}

                  {/* Category tags + edition badges */}
                  <div className="flex flex-wrap gap-1.5 mt-2">
                    {content.categories && content.categories.map((cat) => (
                      <span key={cat} className="badge-gray">{cat}</span>
                    ))}
                    {edition && (
                      <>
                        {edition.max_supply === 0 ? (
                          <span className="badge-blue">Unlimited Edition</span>
                        ) : (
                          <span className="badge-blue">
                            {edition.total_minted}/{edition.max_supply} minted
                          </span>
                        )}
                        {edition.royalty_bps > 0 && (
                          <span className="badge-gray">
                            {(edition.royalty_bps / 100).toFixed(edition.royalty_bps % 100 === 0 ? 0 : 1)}% creator royalty
                          </span>
                        )}
                      </>
                    )}
                  </div>
                </div>

                <div className="text-right flex flex-col items-end gap-2 flex-shrink-0">
                  <p className="text-2xl font-bold text-ara-600 dark:text-ara-400">
                    {content.price_eth} {content.payment_token_symbol ?? "ETH"}
                  </p>
                  {isCreator && (
                    <button
                      onClick={startEditing}
                      className="text-sm text-ara-600 dark:text-ara-400 hover:text-ara-500 font-medium"
                    >
                      Edit listing
                    </button>
                  )}
                </div>
              </div>

              {/* Purchase section */}
              <div className="mt-6 space-y-3">
                {purchaseError && (
                  <div className="alert-error">{purchaseError}</div>
                )}

                {purchaseStep === "done" ? (
                  <div className="alert-success">
                    <p className="font-medium">Purchase successful!</p>

                    {downloadPath && (
                      <div className="mt-2 p-3 bg-emerald-50 dark:bg-emerald-900/20 rounded-lg border border-emerald-200 dark:border-emerald-800/40">
                        <p className="text-sm font-medium mb-1">File downloaded:</p>
                        <p className="text-xs font-mono text-emerald-800 dark:text-emerald-300 break-all mb-2">
                          {downloadPath}
                        </p>
                        <div className="flex gap-2">
                          <button
                            onClick={() => contentId && openDownloadedContent(decodeURIComponent(contentId))}
                            className="text-xs px-3 py-1.5 bg-emerald-600 hover:bg-emerald-700 text-white rounded-lg font-medium transition-colors"
                          >
                            Open File
                          </button>
                          <button
                            onClick={() => contentId && openContentFolder(decodeURIComponent(contentId))}
                            className="text-xs px-3 py-1.5 bg-emerald-100 dark:bg-emerald-800/30 text-emerald-700 dark:text-emerald-300 hover:bg-emerald-200 dark:hover:bg-emerald-800/50 rounded-lg font-medium transition-colors"
                          >
                            Show in Folder
                          </button>
                        </div>
                      </div>
                    )}

                    {purchaseTxHash && (
                      <p className="mt-2 text-sm">
                        <button
                          onClick={() =>
                            openUrl(`https://sepolia.etherscan.io/tx/${purchaseTxHash}`)
                          }
                          className="inline underline font-medium cursor-pointer"
                        >
                          View on Etherscan
                        </button>
                      </p>
                    )}

                    <div className="mt-3 pt-3 border-t border-emerald-200 dark:border-emerald-800/40">
                      {receiptStep === "idle" && (
                        <>
                          <p className="text-xs mb-2 opacity-80">
                            Confirm delivery to help reward the seeder who sent you this file.
                            This is a free signature (no gas cost).
                          </p>
                          <button
                            onClick={handleSignReceipt}
                            className="text-xs px-3 py-1.5 bg-emerald-600 hover:bg-emerald-700 text-white rounded-full font-medium transition-colors"
                          >
                            Sign Delivery Receipt
                          </button>
                        </>
                      )}
                      {receiptStep === "signing" && (
                        <p className="text-xs opacity-80">Signing delivery receipt in wallet...</p>
                      )}
                      {receiptStep === "done" && (
                        <p className="text-xs font-medium">Receipt signed — you're now seeding and earning rewards for this content.</p>
                      )}
                      {receiptStep === "skipped" && (
                        <>
                          <p className="text-xs mb-2 opacity-80">
                            Receipt not signed. Sign to help reward the seeder (gasless).
                          </p>
                          <button
                            onClick={handleSignReceipt}
                            className="text-xs px-3 py-1.5 bg-emerald-600 hover:bg-emerald-700 text-white rounded-full font-medium transition-colors"
                          >
                            Sign Receipt
                          </button>
                        </>
                      )}
                    </div>
                  </div>
                ) : isCreator ? (
                  <div className="alert-info">
                    You are the creator of this listing.
                  </div>
                ) : isSoldOut ? (
                  <div className="alert-warning">
                    <p className="font-medium">Edition Sold Out</p>
                    <p className="text-xs mt-1 opacity-80">
                      All {edition?.max_supply} copies have been minted. Check resale listings below.
                    </p>
                  </div>
                ) : (
                  <div className="space-y-2">
                    <button
                      onClick={handlePurchase}
                      disabled={!canPurchase}
                      className="btn-primary-lg w-full"
                    >
                      {purchaseStep === "idle"
                        ? !isConnected
                          ? "Connect Wallet to Purchase"
                          : `Purchase for ${content.price_eth} ${content.payment_token_symbol ?? "ETH"}`
                        : purchaseStep === "confirming" && downloadProgress
                          ? "Downloading content..."
                          : STEP_LABELS[purchaseStep]}
                    </button>
                    {purchaseStep === "confirming" && downloadProgress && downloadProgress.total > 0 && (
                      <div>
                        <div className="w-full bg-slate-200 dark:bg-slate-700 rounded-full h-2 overflow-hidden">
                          <div
                            className="bg-ara-500 h-2 rounded-full transition-all duration-300"
                            style={{ width: `${Math.min(100, (downloadProgress.received / downloadProgress.total) * 100).toFixed(1)}%` }}
                          />
                        </div>
                        <p className="text-xs text-slate-500 dark:text-slate-400 mt-1 text-center">
                          {fmtBytes(downloadProgress.received)} / {fmtBytes(downloadProgress.total)}{" "}
                          ({((downloadProgress.received / downloadProgress.total) * 100).toFixed(1)}%)
                        </p>
                      </div>
                    )}
                  </div>
                )}
              </div>

              {/* Resale purchase error */}
              {buyResaleError && (
                <div className="mt-4 alert-error text-sm">{buyResaleError}</div>
              )}
            </div>

            {/* Tabbed section */}
            <div className="px-6 pb-6">
              <TabPanel tabs={DETAIL_TABS} defaultTab="details">
                {(activeTab) => {
                  if (activeTab === "details") {
                    return (
                      <div className="space-y-4">
                        {content.description && (
                          <div>
                            <h3 className="text-sm font-semibold text-slate-900 dark:text-slate-100 mb-2">Description</h3>
                            <p className="text-sm text-slate-600 dark:text-slate-400 leading-relaxed whitespace-pre-wrap">
                              {content.description}
                            </p>
                          </div>
                        )}
                        <div className="grid grid-cols-1 gap-2 text-xs">
                          <div className="flex gap-2">
                            <span className="font-semibold text-slate-500 dark:text-slate-500 w-28 flex-shrink-0">Creator</span>
                            <AddressDisplay address={content.creator} className="text-slate-700 dark:text-slate-300" />
                          </div>
                          {content.collaborators && content.collaborators.length > 0 && (
                            <div className="flex gap-2">
                              <span className="font-semibold text-slate-500 dark:text-slate-500 w-28 flex-shrink-0">Revenue Split</span>
                              <div className="space-y-1">
                                {content.collaborators.map((c, i) => (
                                  <div key={i} className="flex items-center gap-2">
                                    <AddressDisplay address={c.wallet} className="text-slate-700 dark:text-slate-300" />
                                    <span className="text-slate-500">({(c.share_bps / 100).toFixed(c.share_bps % 100 === 0 ? 0 : 1)}%)</span>
                                  </div>
                                ))}
                              </div>
                            </div>
                          )}
                          <div className="flex gap-2">
                            <span className="font-semibold text-slate-500 dark:text-slate-500 w-28 flex-shrink-0">Content Hash</span>
                            <span className="font-mono text-slate-700 dark:text-slate-300 break-all">{content.content_hash}</span>
                          </div>
                          <div className="flex gap-2">
                            <span className="font-semibold text-slate-500 dark:text-slate-500 w-28 flex-shrink-0">Content ID</span>
                            <span className="font-mono text-slate-700 dark:text-slate-300 break-all text-[11px]">{content.content_id}</span>
                          </div>
                          {meta?.file_size && meta.file_size > 0 && (
                            <div className="flex gap-2">
                              <span className="font-semibold text-slate-500 dark:text-slate-500 w-28 flex-shrink-0">File Size</span>
                              <span className="text-slate-700 dark:text-slate-300">{fmtBytes(meta.file_size)}</span>
                            </div>
                          )}
                          {meta?.filename && (
                            <div className="flex gap-2">
                              <span className="font-semibold text-slate-500 dark:text-slate-500 w-28 flex-shrink-0">Filename</span>
                              <span className="text-slate-700 dark:text-slate-300">{meta.filename}</span>
                            </div>
                          )}
                        </div>
                      </div>
                    );
                  }

                  if (activeTab === "properties") {
                    return <TraitsGrid traits={buildTraits()} />;
                  }

                  if (activeTab === "activity") {
                    return (
                      <div className="space-y-6">
                        <div>
                          <h3 className="text-sm font-semibold text-slate-900 dark:text-slate-100 mb-3">Price History</h3>
                          <PriceHistoryChart data={priceHistory} />
                        </div>
                        <div>
                          <h3 className="text-sm font-semibold text-slate-900 dark:text-slate-100 mb-3">Sales Activity</h3>
                          <ActivityTable data={priceHistory} />
                        </div>
                      </div>
                    );
                  }

                  if (activeTab === "listings") {
                    return (
                      <div>
                        {resaleListings.length === 0 ? (
                          <div className="text-gray-400 dark:text-gray-500 text-sm text-center py-8">
                            No resale listings
                          </div>
                        ) : (
                          <div className="space-y-2">
                            {resaleListings.map((listing) => {
                              const isBuying = buyResaleSeller === listing.seller;
                              const isOwnListing = address?.toLowerCase() === listing.seller.toLowerCase();
                              return (
                                <div key={listing.seller}
                                  className="flex items-center justify-between p-3 rounded-lg bg-slate-50 dark:bg-slate-800/40 border border-slate-200 dark:border-slate-700">
                                  <div>
                                    <span className="text-sm font-medium text-slate-900 dark:text-slate-100">
                                      {listing.price_eth} ETH
                                    </span>
                                    <span className="text-xs text-slate-500 dark:text-slate-500 ml-2">
                                      from <AddressDisplay address={listing.seller} />
                                    </span>
                                  </div>
                                  {isOwnListing ? (
                                    <span className="text-xs text-slate-400">Your listing</span>
                                  ) : (
                                    <button
                                      onClick={() => handleBuyResale(listing.seller)}
                                      disabled={!isConnected || buyResaleStep !== "idle"}
                                      className="btn-primary text-xs px-3 py-1.5"
                                    >
                                      {isBuying
                                        ? buyResaleStep === "preparing" ? "Preparing..."
                                        : buyResaleStep === "signing" ? "Sign in wallet..."
                                        : buyResaleStep === "confirming" ? "Confirming..."
                                        : buyResaleStep === "done" ? "Purchased!"
                                        : "Buy Resale"
                                        : "Buy Resale"}
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

                  return null;
                }}
              </TabPanel>
            </div>
          </>
        )}
      </div>
    </div>
  );
}

export default ContentDetail;
