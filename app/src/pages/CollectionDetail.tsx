import { useState, useEffect, useCallback } from "react";
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
} from "../lib/tauri";
import type { CollectionInfo, ContentDetail as ContentDetailType } from "../lib/tauri";
import { signAndSendTransactions } from "../lib/transactions";
import AddressDisplay from "../components/AddressDisplay";
import { convertFileSrc } from "@tauri-apps/api/core";
import { getPreviewAsset } from "../lib/tauri";

type Step = "idle" | "preparing" | "signing" | "confirming" | "done";

export default function CollectionDetailPage() {
  const { collectionId } = useParams<{ collectionId: string }>();
  const { address } = useWeb3ModalAccount();
  const { walletProvider } = useWeb3ModalProvider();

  const [info, setInfo] = useState<CollectionInfo | null>(null);
  const [items, setItems] = useState<ContentDetailType[]>([]);
  const [previewSrcs, setPreviewSrcs] = useState<Record<string, string>>({});
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Edit state
  const [editing, setEditing] = useState(false);
  const [editName, setEditName] = useState("");
  const [editDesc, setEditDesc] = useState("");
  const [editBanner, setEditBanner] = useState("");
  const [step, setStep] = useState<Step>("idle");

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
          <div className="h-40 bg-gray-200 dark:bg-gray-800 rounded-lg" />
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
    <div className="p-6 max-w-6xl mx-auto space-y-6">
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
      <div className="flex flex-wrap gap-6">
        <div className="text-center">
          <div className="text-xl font-bold dark:text-white">{info.item_count}</div>
          <div className="text-xs text-gray-500">Items</div>
        </div>
        <div className="text-center">
          <div className="text-xl font-bold dark:text-white">{info.volume_eth}</div>
          <div className="text-xs text-gray-500">Volume (ETH)</div>
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

      {/* Items grid */}
      <h2 className="text-lg font-semibold dark:text-white">Items</h2>
      {items.length === 0 ? (
        <div className="text-gray-400 text-sm py-8 text-center">
          No items in this collection yet.
        </div>
      ) : (
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
          {items.map((item) => {
            const isSoldOut =
              item.max_supply > 0 && item.total_minted >= item.max_supply;
            return (
              <div key={item.content_id} className="relative group">
                <Link
                  to={`/content/${encodeURIComponent(item.content_id)}`}
                  className="card card-hover overflow-hidden block"
                >
                  {/* Thumbnail */}
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
