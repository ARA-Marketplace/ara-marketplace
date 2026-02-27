import { useState, useEffect, useRef } from "react";
import type { CollectionRanking } from "../lib/tauri";
import { getTopCollections } from "../lib/tauri";
import CollectionCard from "./CollectionCard";

export default function FeaturedCarousel() {
  const [collections, setCollections] = useState<CollectionRanking[]>([]);
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    getTopCollections(6).then(setCollections).catch(() => {});
  }, []);

  if (collections.length === 0) return null;

  const scroll = (dir: number) => {
    scrollRef.current?.scrollBy({ left: dir * 280, behavior: "smooth" });
  };

  return (
    <div className="relative">
      <h2 className="text-lg font-semibold dark:text-white mb-3">
        Top Collections
      </h2>
      <div className="relative group">
        <button
          onClick={() => scroll(-1)}
          className="absolute left-0 top-1/2 -translate-y-1/2 z-10 w-8 h-8 rounded-full bg-white/80 dark:bg-gray-800/80 shadow flex items-center justify-center opacity-0 group-hover:opacity-100 transition-opacity"
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
          className="absolute right-0 top-1/2 -translate-y-1/2 z-10 w-8 h-8 rounded-full bg-white/80 dark:bg-gray-800/80 shadow flex items-center justify-center opacity-0 group-hover:opacity-100 transition-opacity"
        >
          <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 5l7 7-7 7" />
          </svg>
        </button>
      </div>
    </div>
  );
}
