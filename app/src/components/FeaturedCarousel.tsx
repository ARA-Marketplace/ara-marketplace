import { useState, useEffect, useRef } from "react";
import { Link } from "react-router-dom";
import type { CollectionRanking } from "../lib/tauri";
import { getTopCollections } from "../lib/tauri";
import CollectionCard from "./CollectionCard";

export default function FeaturedCarousel() {
  const [collections, setCollections] = useState<CollectionRanking[]>([]);
  const scrollRef = useRef<HTMLDivElement>(null);
  const [canScrollLeft, setCanScrollLeft] = useState(false);
  const [canScrollRight, setCanScrollRight] = useState(false);

  useEffect(() => {
    getTopCollections(6).then(setCollections).catch(() => {});
  }, []);

  // Track scroll-ability so we only render arrows when actually scrollable
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const update = () => {
      setCanScrollLeft(el.scrollLeft > 1);
      setCanScrollRight(el.scrollLeft + el.clientWidth < el.scrollWidth - 1);
    };
    update();
    el.addEventListener("scroll", update);
    const ro = new ResizeObserver(update);
    ro.observe(el);
    return () => {
      el.removeEventListener("scroll", update);
      ro.disconnect();
    };
  }, [collections]);

  const scroll = (dir: number) => {
    scrollRef.current?.scrollBy({ left: dir * 280, behavior: "smooth" });
  };

  return (
    <div>
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
        <div className="relative">
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
          {/* Scroll arrows — only rendered when there's actually content to scroll */}
          {canScrollLeft && (
            <button
              onClick={() => scroll(-1)}
              aria-label="Scroll left"
              className="absolute left-2 top-1/2 -translate-y-1/2 z-10 w-8 h-8 rounded-full bg-white/90 dark:bg-slate-800/90 shadow-md flex items-center justify-center hover:bg-white dark:hover:bg-slate-700 transition-colors"
            >
              <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15 19l-7-7 7-7" />
              </svg>
            </button>
          )}
          {canScrollRight && (
            <button
              onClick={() => scroll(1)}
              aria-label="Scroll right"
              className="absolute right-2 top-1/2 -translate-y-1/2 z-10 w-8 h-8 rounded-full bg-white/90 dark:bg-slate-800/90 shadow-md flex items-center justify-center hover:bg-white dark:hover:bg-slate-700 transition-colors"
            >
              <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 5l7 7-7 7" />
              </svg>
            </button>
          )}
        </div>
      )}
    </div>
  );
}
