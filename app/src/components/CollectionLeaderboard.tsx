import { useState, useEffect } from "react";
import { Link } from "react-router-dom";
import type { CollectionRanking } from "../lib/tauri";
import { getTopCollections } from "../lib/tauri";
import AddressDisplay from "./AddressDisplay";

export default function CollectionLeaderboard() {
  const [collections, setCollections] = useState<CollectionRanking[]>([]);

  useEffect(() => {
    getTopCollections(10).then(setCollections).catch(() => {});
  }, []);

  if (collections.length === 0) return null;

  return (
    <div>
      <h2 className="text-lg font-semibold dark:text-white mb-3">
        Top Collections by Volume
      </h2>
      <div className="overflow-x-auto">
        <table className="w-full text-sm">
          <thead>
            <tr className="text-left text-gray-500 dark:text-gray-400 border-b border-gray-200 dark:border-gray-700">
              <th className="pb-2 font-medium w-10">#</th>
              <th className="pb-2 font-medium">Collection</th>
              <th className="pb-2 font-medium text-right">Floor</th>
              <th className="pb-2 font-medium text-right">Volume</th>
              <th className="pb-2 font-medium text-right">Items</th>
            </tr>
          </thead>
          <tbody>
            {collections.map((c, i) => (
              <tr
                key={c.collection_id}
                className="border-b border-gray-100 dark:border-gray-800 hover:bg-gray-50 dark:hover:bg-gray-800/50 transition-colors"
              >
                <td className="py-2 text-gray-400">{i + 1}</td>
                <td className="py-2">
                  <Link
                    to={`/collections/${c.collection_id}`}
                    className="flex items-center gap-2 hover:text-ara-500"
                  >
                    <div className="w-8 h-8 rounded bg-gradient-to-br from-ara-500/30 to-purple-500/30 flex-shrink-0 overflow-hidden">
                      {c.banner_uri && (
                        <img
                          src={c.banner_uri}
                          alt=""
                          className="w-full h-full object-cover"
                          onError={(e) => { (e.target as HTMLImageElement).style.display = "none"; }}
                        />
                      )}
                    </div>
                    <div>
                      <div className="font-medium dark:text-white text-sm truncate max-w-[160px]">
                        {c.name}
                      </div>
                      <div className="text-xs text-gray-400">
                        <AddressDisplay address={c.creator} />
                      </div>
                    </div>
                  </Link>
                </td>
                <td className="py-2 text-right font-mono text-xs">
                  {parseFloat(c.floor_price_eth) > 0
                    ? `${c.floor_price_eth} ETH`
                    : "-"}
                </td>
                <td className="py-2 text-right font-mono text-xs">
                  {c.volume_eth} ETH
                </td>
                <td className="py-2 text-right">{c.item_count}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
