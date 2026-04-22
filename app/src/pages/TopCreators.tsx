import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { convertFileSrc } from "@tauri-apps/api/core";
import {
  getContentDetail,
  getPreviewAsset,
  getTopCreators,
  type TopCreator,
} from "../lib/tauri";

function fmtDate(unix: number): string {
  if (unix === 0) return "—";
  if (unix < 1_000_000_000) return `Block ${unix}`;
  return new Date(unix * 1000).toLocaleDateString();
}

/** Circular avatar that lazily loads the creator's most recent content preview. */
function CreatorAvatar({ creator }: { creator: TopCreator }) {
  const [src, setSrc] = useState<string | null>(null);

  useEffect(() => {
    if (!creator.avatar_content_id) return;
    let cancelled = false;
    (async () => {
      try {
        const detail = await getContentDetail(creator.avatar_content_id!);
        const meta = JSON.parse(detail.metadata_uri);
        if (!meta.main_preview_image?.hash) return;
        const path = await getPreviewAsset({
          contentId: detail.content_id,
          previewHash: meta.main_preview_image.hash,
          filename: meta.main_preview_image.filename,
        });
        if (!cancelled) setSrc(convertFileSrc(path, "localasset"));
      } catch {
        /* gradient fallback */
      }
    })();
    return () => { cancelled = true; };
  }, [creator.avatar_content_id]);

  // Deterministic fallback gradient keyed on the address, so every creator
  // has a distinct "color" even without a published preview yet.
  const hashColor = (addr: string) => {
    let h = 0;
    for (let i = 0; i < addr.length; i++) h = (h << 5) - h + addr.charCodeAt(i);
    return Math.abs(h) % 360;
  };
  const hue = hashColor(creator.address);

  return (
    <div
      className="w-10 h-10 rounded-full overflow-hidden flex-shrink-0 bg-gradient-to-br"
      style={
        src
          ? undefined
          : { background: `linear-gradient(135deg, hsl(${hue} 70% 45%), hsl(${(hue + 40) % 360} 65% 35%))` }
      }
    >
      {src && <img src={src} alt="" className="w-full h-full object-cover" />}
    </div>
  );
}

export default function TopCreators() {
  const [creators, setCreators] = useState<TopCreator[]>([]);
  const [loading, setLoading] = useState(true);
  const [query, setQuery] = useState("");

  useEffect(() => {
    getTopCreators(50)
      .then(setCreators)
      .catch(() => setCreators([]))
      .finally(() => setLoading(false));
  }, []);

  const filtered = query
    ? creators.filter(
        (c) =>
          c.display_name?.toLowerCase().includes(query.toLowerCase()) ||
          c.address.toLowerCase().includes(query.toLowerCase()),
      )
    : creators;

  return (
    <div>
      <div className="mb-6">
        <h1 className="page-title">Top Creators</h1>
        <p className="page-subtitle">
          Ranked by total sales volume. Click a creator to see their full catalog and send a tip.
        </p>
      </div>

      <div className="mb-4">
        <input
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search by name or address…"
          className="input-base max-w-md"
        />
      </div>

      {loading ? (
        <div className="flex items-center justify-center py-16">
          <div className="w-8 h-8 border-2 border-ara-500 border-t-transparent rounded-full animate-spin" />
        </div>
      ) : filtered.length === 0 ? (
        <div className="card p-10 text-center text-slate-500 dark:text-slate-500">
          {creators.length === 0 ? "No creators yet." : "No creators match your search."}
        </div>
      ) : (
        <div className="card overflow-hidden">
          <table className="w-full text-sm">
            <thead className="border-b border-slate-200 dark:border-slate-800">
              <tr>
                {["#", "Creator", "Items", "Sales", "List Volume", "Latest"].map((h, i) => (
                  <th
                    key={h}
                    className={`px-4 py-3 text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-500 ${
                      i === 0 || i === 1 ? "text-left" : "text-right"
                    }`}
                  >
                    {h}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-100 dark:divide-slate-800/60">
              {filtered.map((c, i) => (
                <tr
                  key={c.address}
                  className="hover:bg-slate-50 dark:hover:bg-slate-800/30 transition-colors"
                >
                  <td className="px-4 py-3 text-slate-400 font-mono text-xs">{i + 1}</td>
                  <td className="px-4 py-3">
                    <Link
                      to={`/creator/${c.address}`}
                      className="flex items-center gap-3 hover:text-ara-500 transition-colors"
                    >
                      <CreatorAvatar creator={c} />
                      <div className="flex flex-col">
                        <span className="font-medium text-slate-900 dark:text-slate-100 group-hover:text-ara-500">
                          {c.display_name ?? `${c.address.slice(0, 6)}…${c.address.slice(-4)}`}
                        </span>
                        {c.display_name && (
                          <span className="text-[10px] text-slate-400 font-mono">
                            {c.address.slice(0, 6)}…{c.address.slice(-4)}
                          </span>
                        )}
                      </div>
                    </Link>
                  </td>
                  <td className="px-4 py-3 text-right text-slate-900 dark:text-slate-200">
                    {c.content_count}
                  </td>
                  <td className="px-4 py-3 text-right font-mono text-xs text-slate-900 dark:text-slate-200">
                    {parseFloat(c.total_sales_eth).toFixed(4)} ETH
                  </td>
                  <td className="px-4 py-3 text-right font-mono text-xs text-slate-500 dark:text-slate-400">
                    {parseFloat(c.total_list_volume_eth).toFixed(4)} ETH
                  </td>
                  <td className="px-4 py-3 text-right text-xs text-slate-500 dark:text-slate-400">
                    {fmtDate(c.latest_publish_at)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
