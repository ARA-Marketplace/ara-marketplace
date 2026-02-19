import { useState, useEffect } from "react";
import { Link } from "react-router-dom";
import { searchContent, syncContent, type ContentDetail } from "../lib/tauri";

function Marketplace() {
  const [searchQuery, setSearchQuery] = useState("");
  const [items, setItems] = useState<ContentDetail[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [syncing, setSyncing] = useState(false);
  const [syncMessage, setSyncMessage] = useState<string | null>(null);

  useEffect(() => {
    const timer = setTimeout(() => {
      setLoading(true);
      setError(null);
      searchContent(searchQuery)
        .then(setItems)
        .catch((e) => setError(String(e)))
        .finally(() => setLoading(false));
    }, 300); // debounce search

    return () => clearTimeout(timer);
  }, [searchQuery]);

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
      // Re-run search to show newly synced content
      const results = await searchContent(searchQuery);
      setItems(results);
    } catch (e) {
      setSyncMessage(`Sync failed: ${e}`);
    } finally {
      setSyncing(false);
      // Clear message after a few seconds
      setTimeout(() => setSyncMessage(null), 4000);
    }
  };

  const contentTypeIcon = (type: string) => {
    switch (type) {
      case "game":
        return "🎮";
      case "music":
        return "🎵";
      case "video":
        return "🎬";
      case "document":
        return "📄";
      case "software":
        return "💻";
      default:
        return "📦";
    }
  };

  return (
    <div>
      <div className="mb-8">
        <h1 className="text-3xl font-bold text-gray-900">Marketplace</h1>
        <p className="mt-2 text-gray-600">
          Discover and purchase games, music, videos, and more. Pay with ETH,
          earn by seeding.
        </p>
      </div>

      <div className="mb-6 flex items-center gap-3">
        <input
          type="text"
          placeholder="Search content..."
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          className="w-full max-w-md px-4 py-2 border border-gray-300 rounded-lg focus:ring-2 focus:ring-ara-500 focus:border-transparent"
        />
        <button
          onClick={handleSync}
          disabled={syncing}
          className="px-4 py-2 bg-ara-600 text-white rounded-lg hover:bg-ara-700 disabled:opacity-50 whitespace-nowrap"
        >
          {syncing ? "Syncing..." : "Refresh"}
        </button>
        {syncMessage && (
          <span className="text-sm text-gray-500">{syncMessage}</span>
        )}
      </div>

      {error && (
        <div className="p-4 mb-6 bg-red-50 border border-red-200 rounded-lg text-red-700 text-sm">
          {error}
        </div>
      )}

      {loading ? (
        <div className="text-center text-gray-400 py-12">Loading...</div>
      ) : items.length === 0 ? (
        <div className="bg-white rounded-lg shadow-sm border border-gray-200 p-8 text-center text-gray-400">
          <p className="text-lg">No content published yet</p>
          <p className="mt-2 text-sm">
            Be the first to{" "}
            <Link to="/publish" className="text-ara-600 hover:underline">
              publish content
            </Link>{" "}
            to the Ara marketplace
          </p>
        </div>
      ) : (
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-6">
          {items.map((item) => (
            <Link
              key={item.content_id}
              to={`/content/${encodeURIComponent(item.content_id)}`}
              className="bg-white rounded-lg shadow-sm border border-gray-200 p-5 hover:shadow-md hover:border-ara-300 transition-all"
            >
              <div className="text-3xl mb-3">
                {contentTypeIcon(item.content_type)}
              </div>
              <h3 className="font-semibold text-gray-900 truncate">
                {item.title || "Untitled"}
              </h3>
              {item.description && (
                <p className="text-sm text-gray-500 mt-1 line-clamp-2">
                  {item.description}
                </p>
              )}
              <div className="mt-3 flex items-center justify-between">
                <span className="text-ara-600 font-medium">
                  {item.price_eth} ETH
                </span>
                <span className="text-xs text-gray-400 uppercase">
                  {item.content_type}
                </span>
              </div>
            </Link>
          ))}
        </div>
      )}
    </div>
  );
}

export default Marketplace;
