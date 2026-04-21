import { useState, useEffect, useRef } from "react";
import { Link } from "react-router-dom";
import type { CollectionRanking } from "../lib/tauri";
import { getTopCollections } from "../lib/tauri";
import CollectionCard from "./CollectionCard";

export default function FeaturedCarousel() {
  const [collections, setCollections] = useState<CollectionRanking[]>([]);
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    getTopCollections(6).then(setCollections).catch(() => {});
  }, []);

  const scroll = (dir: number) => {
    scrollRef.current?.scrollBy({ left: dir * 280, behavior: "smooth" });
  };

  return (
    <div className="relative">
      <div className="flex items-center justify-between mb-3">
        <h2 className="text-lg font-semibold dark:text-white">
          Top Collections
        </h2>
        <Link
          to="/collections"
          className="text-sm text-ara-500 hover:text-ara-400 transition-colors"
        >
          View All
        </Link>
      </div>
      {collections.length === 0 ? (
        <div className="flex items-center justify-center py-8 text-gray-400 dark:text-gray-500 text-sm">
          No collections yet. Hit Refresh to sync from the network.
        </div>
      ) : (
        <div className="relative group">
          <button
            onClick={() => scroll(-1)}
            className="absolute left-0 top-1/2 -translate-y-1/2 z-10 w-8 h-8 rounded-full bg-white/80 dark:bg-gray-800/80 shadow flex items-center justify-center invisible group-hover:visible transition-opacity pointer-events-none group-hover:pointer-events-auto"
          >
            <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15 19l-7-7 7-7" />
            </svg>
          </button>
          <div
            ref={scrollRef}
            className="flex gap-4 overflow-x-auto pb-2 scrollbar-hide snap-x"
          >
            {collections.map((c) => (
              <div key={c.collection_id} className="w-[260px] flex-shrink-0 snap-start">
                <CollectionCard collection={c} />
              </div>
            ))}
          </div>
          <button
            onClick={() => scroll(1)}
            className="absolute right-0 top-1/2 -translate-y-1/2 z-10 w-8 h-8 rounded-full bg-white/80 dark:bg-gray-800/80 shadow flex items-center justify-center invisible group-hover:visible transition-opacity pointer-events-none group-hover:pointer-events-auto"
          >
            <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 5l7 7-7 7" />
            </svg>
          </button>
        </div>
      )}
    </div>
  );
}
