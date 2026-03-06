import { useState, useEffect } from "react";
import { Link } from "react-router-dom";
import type { TrendingItem } from "../lib/tauri";
import { getTrendingContent } from "../lib/tauri";
import { TYPE_ICONS } from "../lib/format";

export default function TrendingGrid() {
  const [items, setItems] = useState<TrendingItem[]>([]);

  useEffect(() => {
    getTrendingContent(4).then(setItems).catch(() => {});
  }, []);

  if (items.length === 0) return null;

  return (
    <div>
      <h2 className="text-lg font-semibold dark:text-white mb-3">
        Trending Now
      </h2>
      <div className="grid grid-cols-2 lg:grid-cols-4 gap-3">
        {items.map((item) => (
          <Link
            key={item.content_id}
            to={`/content/${encodeURIComponent(item.content_id)}`}
            className="card-hover flex flex-col overflow-hidden group"
          >
            <div className="aspect-video w-full relative overflow-hidden bg-slate-900 flex items-center justify-center">
              <span className="text-3xl select-none">
                {TYPE_ICONS[item.content_type] ?? "📦"}
              </span>
              <span className="absolute top-1.5 left-1.5 px-1.5 py-0.5 bg-amber-500/90 text-white text-[9px] font-bold uppercase tracking-wider rounded-full">
                Trending
              </span>
              <span className="absolute top-1.5 right-1.5 px-1.5 py-0.5 bg-black/50 backdrop-blur-sm text-white text-[9px] font-semibold uppercase tracking-wider rounded-full">
                {item.content_type}
              </span>
            </div>
            <div className="p-3 flex flex-col gap-1">
              <h3 className="font-semibold text-slate-900 dark:text-slate-100 truncate text-xs">
                {item.title}
              </h3>
              <div className="flex items-center justify-between">
                <span className="text-ara-600 dark:text-ara-400 font-bold text-xs">
                  {item.price_eth} ETH
                </span>
                <span className="text-[10px] text-slate-400">
                  {item.recent_sales} sale{item.recent_sales !== 1 ? "s" : ""}
                </span>
              </div>
            </div>
          </Link>
        ))}
      </div>
    </div>
  );
}
