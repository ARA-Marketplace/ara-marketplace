import { useState, useEffect, useCallback } from "react";
import { Link } from "react-router-dom";
import {
  getLibrary,
  startSeeding,
  stopSeeding,
  openDownloadedContent,
  openContentFolder,
  type LibraryItem,
} from "../lib/tauri";
import { useWeb3ModalAccount } from "@web3modal/ethers/react";
import { openUrl } from "@tauri-apps/plugin-opener";

function Library() {
  const { isConnected } = useWeb3ModalAccount();
  const [items, setItems] = useState<LibraryItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [togglingId, setTogglingId] = useState<string | null>(null);
  const [openingId, setOpeningId] = useState<string | null>(null);

  const fetchLibrary = useCallback(() => {
    if (!isConnected) {
      setItems([]);
      setLoading(false);
      return;
    }

    setLoading(true);
    getLibrary()
      .then(setItems)
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  }, [isConnected]);

  useEffect(() => {
    fetchLibrary();
  }, [fetchLibrary]);

  const toggleSeeding = async (item: LibraryItem) => {
    setTogglingId(item.content_id);
    setError(null);
    try {
      if (item.is_seeding) {
        await stopSeeding(item.content_id);
      } else {
        await startSeeding(item.content_id);
      }
      fetchLibrary();
    } catch (e) {
      setError(String(e));
    } finally {
      setTogglingId(null);
    }
  };

  const openFile = async (item: LibraryItem) => {
    setOpeningId(item.content_id);
    setError(null);
    try {
      await openDownloadedContent(item.content_id);
    } catch (e) {
      setError(String(e));
    } finally {
      setOpeningId(null);
    }
  };

  const openFolder = async (item: LibraryItem) => {
    setError(null);
    try {
      await openContentFolder(item.content_id);
    } catch (e) {
      setError(String(e));
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

  const formatDate = (timestamp: number) => {
    return new Date(timestamp * 1000).toLocaleDateString(undefined, {
      year: "numeric",
      month: "short",
      day: "numeric",
    });
  };

  return (
    <div>
      <h1 className="text-3xl font-bold text-gray-900">Your Library</h1>
      <p className="mt-2 text-gray-600 mb-8">
        Content you've purchased. Toggle seeding to earn ETH rewards.
      </p>

      {!isConnected && (
        <div className="p-4 bg-yellow-50 border border-yellow-200 rounded-lg text-yellow-700 text-sm mb-6">
          Connect your wallet to view your library.
        </div>
      )}

      {error && (
        <div className="p-4 bg-red-50 border border-red-200 rounded-lg text-red-700 text-sm mb-6">
          {error}
        </div>
      )}

      {loading ? (
        <div className="text-center text-gray-400 py-12">Loading...</div>
      ) : items.length === 0 ? (
        <div className="bg-white rounded-lg shadow-sm border border-gray-200 p-8 text-center text-gray-400">
          <p className="text-lg">No purchases yet</p>
          <p className="mt-2 text-sm">
            Browse the{" "}
            <Link to="/" className="text-ara-600 hover:underline">
              marketplace
            </Link>{" "}
            to find content to purchase
          </p>
        </div>
      ) : (
        <div className="space-y-3">
          {items.map((item) => (
            <div
              key={item.content_id}
              className="bg-white rounded-lg shadow-sm border border-gray-200 p-4 flex items-center gap-4"
            >
              <div className="text-2xl">
                {contentTypeIcon(item.content_type)}
              </div>
              <div className="flex-1 min-w-0">
                <Link
                  to={`/content/${encodeURIComponent(item.content_id)}`}
                  className="font-medium text-gray-900 hover:text-ara-600 truncate block"
                >
                  {item.title}
                </Link>
                <p className="text-xs text-gray-400">
                  Purchased {formatDate(item.purchased_at)}
                  {item.tx_hash && item.tx_hash !== "0x0" && (
                    <button
                      onClick={() => openUrl(`https://sepolia.etherscan.io/tx/${item.tx_hash}`)}
                      className="ml-2 inline-flex items-center gap-1 text-ara-600 hover:text-ara-700 cursor-pointer"
                      title="View on Etherscan"
                    >
                      <svg className="w-3 h-3" viewBox="0 0 293.775 293.671" fill="currentColor">
                        <path d="M61.342,147.035c0-4.94,4.018-8.947,8.974-8.947h42.144c4.955,0,8.974,4.007,8.974,8.947v78.592c1.6-.56,3.6-1.2,5.8-1.92a9.157,9.157,0,0,0,6.353-8.726V120.276c0-4.945,4.014-8.952,8.97-8.952h42.16c4.955,0,8.974,4.007,8.974,8.952v85.194a9.122,9.122,0,0,0,5.611-3.043,9.157,9.157,0,0,0,2.189-5.955V93.166c0-4.94,4.018-8.947,8.974-8.947h42.144c4.955,0,8.974,4.007,8.974,8.947v87.194c0,.3-.019.592-.038.884,24.563-17.4,49.3-38.944,49.3-77.208,0-81.08-81.08-146.835-146.835-146.835S.1,22.955.1,104.035c0,62.467,43.555,105.749,82.535,131.848a9.086,9.086,0,0,0,5.1,1.557,9.161,9.161,0,0,0,9.145-9.145V147.035h-35.54Z"/>
                      </svg>
                      tx
                    </button>
                  )}
                  {item.download_path && (
                    <span className="ml-2 text-green-600">• Downloaded</span>
                  )}
                </p>
              </div>
              <div className="flex items-center gap-2">
                {item.download_path ? (
                  <>
                    <button
                      onClick={() => openFile(item)}
                      disabled={openingId === item.content_id}
                      className="text-sm px-3 py-1.5 rounded-full font-medium transition-colors bg-blue-100 text-blue-700 hover:bg-blue-200 disabled:opacity-50"
                    >
                      {openingId === item.content_id ? "Opening..." : "Open File"}
                    </button>
                    <button
                      onClick={() => openFolder(item)}
                      className="text-sm px-3 py-1.5 rounded-full font-medium transition-colors bg-gray-100 text-gray-700 hover:bg-gray-200"
                    >
                      Open Folder
                    </button>
                  </>
                ) : (
                  <span className="text-xs text-gray-400 px-3 py-1.5">
                    Not downloaded
                  </span>
                )}
                <button
                  onClick={() => toggleSeeding(item)}
                  disabled={togglingId === item.content_id}
                  className={`text-sm px-3 py-1.5 rounded-full font-medium transition-colors disabled:opacity-50 ${
                    item.is_seeding
                      ? "bg-green-100 text-green-700 hover:bg-green-200"
                      : "bg-gray-100 text-gray-500 hover:bg-gray-200"
                  }`}
                >
                  {togglingId === item.content_id
                    ? "..."
                    : item.is_seeding
                      ? "Seeding"
                      : "Start Seeding"}
                </button>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

export default Library;
