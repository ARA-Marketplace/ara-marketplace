import { useState, useEffect } from "react";
import { useParams, Link } from "react-router-dom";
import { convertFileSrc } from "@tauri-apps/api/core";
import {
  getContentDetail,
  purchaseContent,
  confirmPurchase,
  updateContent,
  confirmUpdateContent,
  broadcastDeliveryReceipt,
  getMarketplaceAddress,
  getPreviewAsset,
  type ContentDetail as ContentDetailType,
  type ContentMetadataV2,
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

const CONTENT_CATEGORIES = [
  "Action", "RPG", "Strategy", "Puzzle", "Adventure",
  "Simulation", "Sports", "Horror", "Platformer", "Shooter",
  "Indie", "Educational", "Music", "Other",
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

  // Preview carousel state
  const [carouselItems, setCarouselItems] = useState<CarouselItem[]>([]);
  const [carouselIndex, setCarouselIndex] = useState(0);

  // Edit state
  const [editing, setEditing] = useState(false);
  const [editTitle, setEditTitle] = useState("");
  const [editDescription, setEditDescription] = useState("");
  const [editContentType, setEditContentType] = useState("");
  const [editPriceEth, setEditPriceEth] = useState("");
  const [editCategories, setEditCategories] = useState<string[]>([]);
  const [editStep, setEditStep] = useState<EditStep>("idle");
  const [editError, setEditError] = useState<string | null>(null);

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
    if (!contentId || !isConnected) return;
    setPurchaseError(null);
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
        setPurchaseStep("confirming");
        await confirmPurchase(result.content_id, txHash);
      } else {
        setPurchaseStep("confirming");
        await confirmPurchase(result.content_id, txHash);
      }

      setPurchaseTxHash(txHash !== "0x0" ? txHash : null);
      setPurchaseStep("done");

      // Auto-trigger receipt signing — prompts the wallet immediately so the
      // buyer doesn't need to find and click a separate button.
      // Uses a short delay to let the "Purchase successful!" UI render first.
      setTimeout(() => handleSignReceipt(), 500);
    } catch (err) {
      setPurchaseError(String(err));
      setPurchaseStep("idle");
    }
  };

  const handleSignReceipt = async () => {
    if (!content || !address || !walletProvider) return;
    setReceiptStep("signing");
    try {
      const marketplaceAddr = await getMarketplaceAddress();
      if (!marketplaceAddr) throw new Error("Marketplace not configured");
      const timestamp = Math.floor(Date.now() / 1000);
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
          timestamp,
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
      });
      setReceiptStep("done");
    } catch {
      setReceiptStep("skipped");
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

  if (loading) {
    return (
      <div className="flex items-center justify-center py-20">
        <div className="text-center space-y-3">
          <div className="w-8 h-8 border-2 border-ara-500 border-t-transparent rounded-full animate-spin mx-auto" />
          <p className="text-sm text-slate-400 dark:text-slate-600">Loading content…</p>
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

  const canPurchase = isConnected && purchaseStep === "idle" && content.active && !isCreator;

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
                {CONTENT_CATEGORIES.map((cat) => {
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
                  {/* Main 16:9 frame */}
                  <div className="relative w-full aspect-video flex items-center justify-center">
                    {current.loading ? (
                      <div className="flex flex-col items-center gap-2">
                        <div className="w-6 h-6 border-2 border-ara-500 border-t-transparent rounded-full animate-spin" />
                        <span className="text-slate-500 text-xs">Loading preview…</span>
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

                    {/* Prev / Next arrows */}
                    {carouselItems.length > 1 && (
                      <>
                        <button
                          onClick={() =>
                            setCarouselIndex((i) => (i - 1 + carouselItems.length) % carouselItems.length)
                          }
                          className="absolute left-2 top-1/2 -translate-y-1/2 bg-black/60 hover:bg-black/80 text-white w-8 h-8 rounded-full flex items-center justify-center transition-colors text-lg leading-none"
                          aria-label="Previous"
                        >
                          ‹
                        </button>
                        <button
                          onClick={() =>
                            setCarouselIndex((i) => (i + 1) % carouselItems.length)
                          }
                          className="absolute right-2 top-1/2 -translate-y-1/2 bg-black/60 hover:bg-black/80 text-white w-8 h-8 rounded-full flex items-center justify-center transition-colors text-lg leading-none"
                          aria-label="Next"
                        >
                          ›
                        </button>
                      </>
                    )}
                  </div>

                  {/* Filmstrip thumbnail strip */}
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
                              {item.loading ? "…" : "✕"}
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
                  </div>
                  <p className="text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-500 mt-1">
                    {content.content_type}
                  </p>

                  {/* Category tags */}
                  {content.categories && content.categories.length > 0 && (
                    <div className="flex flex-wrap gap-1.5 mt-2">
                      {content.categories.map((cat) => (
                        <span key={cat} className="badge-gray">{cat}</span>
                      ))}
                    </div>
                  )}
                </div>

                <div className="text-right flex flex-col items-end gap-2 flex-shrink-0">
                  <p className="text-2xl font-bold text-ara-600 dark:text-ara-400">
                    {content.price_eth} ETH
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

              {content.description && (
                <p className="mt-4 text-slate-600 dark:text-slate-400 leading-relaxed">
                  {content.description}
                </p>
              )}

              <div className="mt-4 pt-4 border-t border-slate-100 dark:border-slate-800 grid grid-cols-1 gap-2 text-xs">
                <div className="flex gap-2">
                  <span className="font-semibold text-slate-500 dark:text-slate-500 w-28 flex-shrink-0">Creator</span>
                  <span className="font-mono text-slate-700 dark:text-slate-300 break-all">{content.creator}</span>
                </div>
                <div className="flex gap-2">
                  <span className="font-semibold text-slate-500 dark:text-slate-500 w-28 flex-shrink-0">Content Hash</span>
                  <span className="font-mono text-slate-700 dark:text-slate-300 break-all">{content.content_hash}</span>
                </div>
              </div>

              <div className="mt-6 space-y-3">
                {!isConnected && purchaseStep === "idle" && (
                  <div className="alert-warning">
                    Connect your wallet to purchase this content.
                  </div>
                )}

                {purchaseError && (
                  <div className="alert-error">{purchaseError}</div>
                )}

                {purchaseStep === "done" ? (
                  <div className="alert-success">
                    <p className="font-medium">Purchase successful!</p>
                    <p className="mt-1 text-sm">
                      Check your{" "}
                      <Link to="/library" className="underline font-medium">
                        Library
                      </Link>{" "}
                      to download your content.
                      {purchaseTxHash && (
                        <>
                          {" "}
                          <button
                            onClick={() =>
                              openUrl(`https://sepolia.etherscan.io/tx/${purchaseTxHash}`)
                            }
                            className="inline underline font-medium cursor-pointer"
                          >
                            View on Etherscan ↗
                          </button>
                        </>
                      )}
                    </p>

                    {receiptStep === "idle" && (
                      <p className="mt-2 text-xs opacity-80">Requesting delivery receipt signature…</p>
                    )}
                    {receiptStep === "signing" && (
                      <p className="mt-2 text-xs opacity-80">Waiting for signature in wallet…</p>
                    )}
                    {receiptStep === "done" && (
                      <p className="mt-2 text-xs font-medium">Receipt signed — seeder delivery verified.</p>
                    )}
                    {receiptStep === "skipped" && (
                      <div className="mt-3 pt-3 border-t border-emerald-200 dark:border-emerald-800/40">
                        <p className="text-xs mb-2 opacity-80">
                          Receipt not signed. Sign to help reward the seeder (gasless).
                        </p>
                        <button
                          onClick={handleSignReceipt}
                          className="text-xs px-3 py-1.5 bg-emerald-600 hover:bg-emerald-700 text-white rounded-full font-medium transition-colors"
                        >
                          Sign Receipt
                        </button>
                      </div>
                    )}
                  </div>
                ) : isCreator ? (
                  <div className="alert-info">
                    You are the creator of this listing.
                  </div>
                ) : (
                  <button
                    onClick={handlePurchase}
                    disabled={!canPurchase}
                    className="btn-primary-lg w-full"
                  >
                    {purchaseStep === "idle"
                      ? `Purchase for ${content.price_eth} ETH`
                      : STEP_LABELS[purchaseStep]}
                  </button>
                )}
              </div>
            </div>
          </>
        )}
      </div>
    </div>
  );
}

export default ContentDetail;
