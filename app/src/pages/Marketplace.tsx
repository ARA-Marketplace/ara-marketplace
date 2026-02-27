import { useState, useEffect, useCallback } from "react";
import { Link } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import { convertFileSrc } from "@tauri-apps/api/core";
import {
  searchContent, syncContent, getPreviewAsset,
  type ContentDetail, type ContentMetadataV2,
} from "../lib/tauri";
import { IconSearch, IconRefresh } from "../components/Icons";

const TYPE_GRADIENTS: Record<string, string> = {
  game:     "from-violet-700 via-purple-700 to-indigo-800",
  music:    "from-pink-700 via-rose-600 to-purple-800",
  video:    "from-orange-600 via-red-600 to-rose-800",
  document: "from-teal-700 via-cyan-700 to-sky-800",
  software: "from-emerald-700 via-teal-700 to-cyan-800",
  other:    "from-slate-600 via-slate-700 to-gray-800",
};

const TYPE_ICONS: Record<string, string> = {
  game: "🎮", music: "🎵", video: "🎬",
  document: "📄", software: "💾", other: "📦",
};

function Marketplace() {
  const [searchQuery, setSearchQuery] = useState("");
  const [items, setItems] = useState<ContentDetail[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [syncing, setSyncing] = useState(false);
  const [syncMessage, setSyncMessage] = useState<string | null>(null);
  // contentId → preview image src (loaded lazily)
  const [previewSrcs, setPreviewSrcs] = useState<Record<string, string>>({});

  const fetchContent = useCallback((query: string) => {
    setLoading(true);
    setError(null);
    searchContent(query)
      .then(setItems)
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => {
    const timer = setTimeout(() => fetchContent(searchQuery), 300);
    return () => clearTimeout(timer);
  }, [searchQuery, fetchContent]);

  useEffect(() => {
    const unlisten = listen("content-synced", () => fetchContent(searchQuery));
    return () => { unlisten.then((f) => f()); };
  }, [searchQuery, fetchContent]);

  // Lazily load preview images whenever the item list changes
  useEffect(() => {
    if (items.length === 0) return;
    setPreviewSrcs({});

    items.forEach((item) => {
      let meta: ContentMetadataV2;
      try {
        meta = JSON.parse(item.metadata_uri);
      } catch {
        return;
      }
      if (!meta.main_preview_image?.hash) return;

      getPreviewAsset({
        contentId: item.content_id,
        previewHash: meta.main_preview_image.hash,
        filename: meta.main_preview_image.filename,
      })
        .then((localPath) => convertFileSrc(localPath, "localasset"))
        .then((src) =>
          setPreviewSrcs((prev) => ({ ...prev, [item.content_id]: src }))
        )
        .catch(() => {/* no preview cached — fall back to gradient */});
    });
  }, [items]);

  const handleSync = async () => {
    setSyncing(true);
    setSyncMessage(null);
    try {
      const result = await syncContent();
      setSyncMessage(
        result.new_content > 0
          ? `Synced ${result.new_content} new item${result.new_content === 1 ? "" : "s"}`
          : "Up to date"
      );
      setItems(await searchContent(searchQuery));
    } catch (e) {
      setSyncMessage(`Sync failed: ${e}`);
    } finally {
      setSyncing(false);
      setTimeout(() => setSyncMessage(null), 4000);
    }
  };

  return (
    <div>
      <div className="mb-6">
        <h1 className="page-title">Marketplace</h1>
        <p className="page-subtitle">
          Discover and purchase content. Pay with ETH — seeders earn rewards.
        </p>
      </div>

      <div className="mb-6 flex items-center gap-3">
        <div className="relative flex-1 max-w-sm">
          <IconSearch className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-slate-400 pointer-events-none" />
          <input
            type="text"
            placeholder="Search content..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="input-base pl-9"
          />
        </div>
        <button onClick={handleSync} disabled={syncing} className="btn-secondary gap-2">
          <IconRefresh className={`w-4 h-4 ${syncing ? "animate-spin" : ""}`} />
          {syncing ? "Syncing…" : "Refresh"}
        </button>
        {syncMessage && (
          <span className="text-xs text-slate-500 dark:text-slate-400">{syncMessage}</span>
        )}
      </div>

      {error && <div className="alert-error mb-6">{error}</div>}

      {loading ? (
        <div className="flex items-center justify-center py-20">
          <div className="text-center space-y-3">
            <div className="w-8 h-8 border-2 border-ara-500 border-t-transparent rounded-full animate-spin mx-auto" />
            <p className="text-sm text-slate-400 dark:text-slate-600">Loading content…</p>
          </div>
        </div>
      ) : items.length === 0 ? (
        <div className="card p-12 text-center">
          <p className="text-slate-400 dark:text-slate-600 text-lg mb-2">No content found</p>
          <p className="text-sm text-slate-500 dark:text-slate-600">
            Be the first to{" "}
            <Link to="/publish" className="text-ara-500 hover:text-ara-400 underline">
              publish content
            </Link>{" "}
            to the Ara marketplace.
          </p>
        </div>
      ) : (
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
          {items.map((item) => {
            const gradient = TYPE_GRADIENTS[item.content_type] ?? TYPE_GRADIENTS.other;
            const icon = TYPE_ICONS[item.content_type] ?? "📦";
            const previewSrc = previewSrcs[item.content_id];

            return (
              <Link
                key={item.content_id}
                to={`/content/${encodeURIComponent(item.content_id)}`}
                className="card-hover flex flex-col overflow-hidden group"
              >
                {/* Thumbnail: preview image if available, else gradient */}
                <div className="aspect-video w-full relative overflow-hidden bg-slate-900">
                  {previewSrc ? (
                    <img
                      src={previewSrc}
                      alt={item.title}
                      className="w-full h-full object-cover group-hover:scale-[1.03] transition-transform duration-300"
                    />
                  ) : (
                    <div
                      className={`w-full h-full bg-gradient-to-br ${gradient} flex items-center justify-center`}
                    >
                      <span className="text-4xl drop-shadow-lg select-none">{icon}</span>
                      <div className="absolute inset-0 bg-black/10 group-hover:bg-black/0 transition-colors" />
                    </div>
                  )}
                  {/* Type badge — always shown */}
                  <span className="absolute top-2 left-2 px-2 py-0.5 bg-black/50 backdrop-blur-sm text-white text-[10px] font-semibold uppercase tracking-wider rounded-full">
                    {item.content_type}
                  </span>
                  {/* Sold Out / Resale Available badge for limited editions */}
                  {item.max_supply > 0 && item.total_minted >= item.max_supply && (
                    item.resale_count > 0 ? (
                      <span className="absolute top-2 right-2 px-2 py-0.5 bg-amber-600/80 backdrop-blur-sm text-white text-[10px] font-semibold uppercase tracking-wider rounded-full">
                        Resale Available
                      </span>
                    ) : (
                      <span className="absolute top-2 right-2 px-2 py-0.5 bg-red-600/80 backdrop-blur-sm text-white text-[10px] font-semibold uppercase tracking-wider rounded-full">
                        Sold Out
                      </span>
                    )
                  )}
                </div>

                {/* Card body */}
                <div className="p-4 flex flex-col gap-1 flex-1">
                  <h3 className="font-semibold text-slate-900 dark:text-slate-100 truncate text-sm">
                    {item.title || "Untitled"}
                  </h3>
                  {item.description && (
                    <p className="text-xs text-slate-500 dark:text-slate-500 line-clamp-2 leading-relaxed">
                      {item.description}
                    </p>
                  )}
                  <div className="mt-auto pt-2 flex items-center justify-between">
                    {item.max_supply > 0 && item.total_minted >= item.max_supply && item.min_resale_price_eth ? (
                      <span className="text-ara-600 dark:text-ara-400 font-bold text-sm">
                        {item.min_resale_price_eth} ETH
                        <span className="text-[10px] font-normal text-slate-400 ml-1">resale</span>
                      </span>
                    ) : (
                      <span className="text-ara-600 dark:text-ara-400 font-bold text-sm">
                        {item.price_eth} ETH
                      </span>
                    )}
                    {item.categories && item.categories.length > 0 && (
                      <span className="badge-gray text-[10px]">{item.categories[0]}</span>
                    )}
                  </div>
                </div>
              </Link>
            );
          })}
        </div>
      )}
    </div>
  );
}

export default Marketplace;
