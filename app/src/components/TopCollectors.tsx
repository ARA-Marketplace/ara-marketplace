import { useState, useEffect } from "react";
import type { CollectorRanking } from "../lib/tauri";
import { getTopCollectors } from "../lib/tauri";
import AddressDisplay from "./AddressDisplay";

export default function TopCollectors() {
  const [collectors, setCollectors] = useState<CollectorRanking[]>([]);

  useEffect(() => {
    getTopCollectors(10).then(setCollectors).catch(() => {});
  }, []);

  if (collectors.length === 0) return null;

  return (
    <div>
      <h2 className="text-lg font-semibold dark:text-white mb-3">
        Top Collectors
      </h2>
      <div className="space-y-2">
        {collectors.map((c, i) => (
          <div
            key={c.address}
            className="flex items-center justify-between py-2 border-b border-gray-100 dark:border-gray-800 last:border-0"
          >
            <div className="flex items-center gap-2">
              <span className="text-xs text-gray-400 w-5">{i + 1}</span>
              <AddressDisplay address={c.address} />
            </div>
            <div className="text-right">
              <div className="text-xs font-mono dark:text-white">
                {c.total_spent_eth} ETH
              </div>
              <div className="text-[10px] text-gray-400">
                {c.purchase_count} purchases
              </div>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
