import { useState, useEffect } from "react";
import { Link } from "react-router-dom";
import { getAllCollections } from "../lib/tauri";
import type { CollectionInfo } from "../lib/tauri";
import AddressDisplay from "../components/AddressDisplay";

const PAGE_SIZE = 20;

export default function Collections() {
  const [collections, setCollections] = useState<CollectionInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [hasMore, setHasMore] = useState(true);

  const fetchPage = (offset: number) => {
    setLoading(true);
    getAllCollections(PAGE_SIZE, offset)
      .then((results) => {
        if (offset === 0) {
          setCollections(results);
        } else {
          setCollections((prev) => [...prev, ...results]);
        }
        setHasMore(results.length === PAGE_SIZE);
      })
      .catch(() => {})
      .finally(() => setLoading(false));
  };

  useEffect(() => {
    fetchPage(0);
  }, []);

  return (
    <div>
      <div className="mb-6">
        <h1 className="page-title">Collections</h1>
        <p className="page-subtitle">
          Browse all collections on the Ara marketplace.
        </p>
      </div>

      {collections.length === 0 && !loading ? (
        <div className="card p-12 text-center">
          <p className="text-slate-400 dark:text-slate-600 text-lg mb-2">
            No collections yet
          </p>
          <p className="text-sm text-slate-500 dark:text-slate-600">
            <Link to="/publish" className="text-ara-500 hover:text-ara-400 underline">
              Publish content
            </Link>{" "}
            and create a collection to get started.
          </p>
        </div>
      ) : (
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
          {collections.map((c) => (
            <Link
              key={c.collection_id}
              to={`/collections/${c.collection_id}`}
              className="card card-hover overflow-hidden block"
            >
              {/* Banner */}
              <div className="h-28 bg-gradient-to-br from-ara-600/30 to-purple-600/30 relative overflow-hidden">
                {c.banner_uri && (
                  <img
                    src={c.banner_uri}
                    alt=""
                    className="w-full h-full object-cover"
                    onError={(e) => {
                      (e.target as HTMLImageElement).style.display = "none";
                    }}
                  />
                )}
              </div>
              {/* Info */}
              <div className="p-4">
                <h3 className="font-semibold text-sm dark:text-white truncate">
                  {c.name}
                </h3>
                <div className="text-xs text-gray-500 mt-1">
                  by <AddressDisplay address={c.creator} />
                </div>
                <div className="flex items-center gap-4 mt-3 text-xs text-gray-400">
                  <span>{c.item_count} items</span>
                  <span>{c.volume_eth} ETH vol</span>
                </div>
              </div>
            </Link>
          ))}
        </div>
      )}

      {loading && (
        <div className="flex items-center justify-center py-10">
          <div className="w-8 h-8 border-2 border-ara-500 border-t-transparent rounded-full animate-spin" />
        </div>
      )}

      {!loading && hasMore && collections.length > 0 && (
        <div className="text-center mt-6">
          <button
            onClick={() => fetchPage(collections.length)}
            className="btn-secondary"
          >
            Load More
          </button>
        </div>
      )}
    </div>
  );
}
