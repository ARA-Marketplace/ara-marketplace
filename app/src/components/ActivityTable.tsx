import type { PricePoint } from "../lib/tauri";
import AddressDisplay from "./AddressDisplay";

interface ActivityTableProps {
  data: PricePoint[];
  className?: string;
}

export default function ActivityTable({
  data,
  className,
}: ActivityTableProps) {
  if (data.length === 0) {
    return (
      <div className={`text-gray-400 dark:text-gray-500 text-sm text-center py-8 ${className ?? ""}`}>
        No activity yet
      </div>
    );
  }

  return (
    <div className={`overflow-x-auto ${className ?? ""}`}>
      <table className="w-full text-sm">
        <thead>
          <tr className="text-left text-gray-500 dark:text-gray-400 border-b border-gray-200 dark:border-gray-700">
            <th className="pb-2 font-medium">Event</th>
            <th className="pb-2 font-medium">Price</th>
            <th className="pb-2 font-medium">Buyer</th>
            <th className="pb-2 font-medium">Tx</th>
          </tr>
        </thead>
        <tbody>
          {data.map((p, i) => (
            <tr
              key={i}
              className="border-b border-gray-100 dark:border-gray-800"
            >
              <td className="py-2">
                <span
                  className={`text-xs px-2 py-0.5 rounded-full ${
                    p.is_resale
                      ? "bg-amber-100 text-amber-700 dark:bg-amber-900/30 dark:text-amber-400"
                      : "bg-purple-100 text-purple-700 dark:bg-purple-900/30 dark:text-purple-400"
                  }`}
                >
                  {p.is_resale ? "Resale" : "Sale"}
                </span>
              </td>
              <td className="py-2 font-mono">{p.price_eth} ETH</td>
              <td className="py-2">
                <AddressDisplay address={p.buyer} />
              </td>
              <td className="py-2">
                {p.tx_hash && (
                  <a
                    href={`https://sepolia.etherscan.io/tx/${p.tx_hash}`}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="text-ara-500 hover:underline font-mono text-xs"
                  >
                    {p.tx_hash.slice(0, 8)}...
                  </a>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
