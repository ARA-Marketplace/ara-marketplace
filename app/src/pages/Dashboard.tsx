import { useState, useEffect, useCallback } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  getSeederStats,
  getMarketplaceOverview,
  getAraPriceUsd,
  type SeederStats,
  type MarketplaceOverview,
} from "../lib/tauri";
import { useWeb3ModalAccount } from "@web3modal/ethers/react";

function formatBytes(bytes: number) {
  if (bytes === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${units[i]}`;
}

function fmtAra(val: string): string {
  const n = parseFloat(val);
  if (isNaN(n)) return val;
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(2)}K`;
  return n.toFixed(2);
}

function fmtUsdPrice(n: number): string {
  if (n <= 0) return "—";
  if (n >= 1) return `$${n.toFixed(2)}`;
  if (n >= 0.01) return `$${n.toFixed(4)}`;
  // Sub-penny pricing (e.g., ARA at $0.00012316) — show more significant digits
  return `$${n.toFixed(6)}`;
}

/**
 * Render a crypto amount with adaptive precision so tiny values don't round to "0.000".
 * Examples:
 *   0.000250 ETH → "0.00025 ETH"  (seeder rewards paid, early testnet)
 *   0.042    ETH → "0.0420 ETH"
 *   3.14     ETH → "3.140 ETH"
 */
function fmtCrypto(value: string, symbol: string): string {
  const n = parseFloat(value);
  if (!Number.isFinite(n) || n === 0) return `0 ${symbol}`;
  if (n >= 1) return `${n.toFixed(3)} ${symbol}`;
  if (n >= 0.01) return `${n.toFixed(4)} ${symbol}`;
  if (n >= 0.0001) return `${n.toFixed(6)} ${symbol}`;
  // Very small — show up to 8 decimals, stripping trailing zeros
  return `${n.toFixed(8).replace(/0+$/, "").replace(/\.$/, "")} ${symbol}`;
}

function Dashboard() {
  const { isConnected } = useWeb3ModalAccount();
  const [stats, setStats] = useState<SeederStats[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [overview, setOverview] = useState<MarketplaceOverview | null>(null);
  const [araPriceUsd, setAraPriceUsd] = useState<number>(0);

  useEffect(() => {
    // Network stats don't require a connected wallet — load regardless
    getMarketplaceOverview().then(setOverview).catch(() => {});
    getAraPriceUsd().then(setAraPriceUsd).catch(() => {});
  }, []);

  const fetchStats = useCallback(() => {
    getSeederStats()
      .then(setStats)
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => {
    if (!isConnected) { setStats([]); setLoading(false); return; }
    fetchStats();
    const interval = setInterval(fetchStats, 10000);
    return () => clearInterval(interval);
  }, [isConnected, fetchStats]);

  useEffect(() => {
    const unlisten = listen("seeder-stats-updated", () => {
      if (isConnected) fetchStats();
    });
    return () => { unlisten.then((f) => f()); };
  }, [isConnected, fetchStats]);

  const activeCount = stats.filter((s) => s.is_active).length;
  const totalBytesServed = stats.reduce((sum, s) => sum + s.bytes_served, 0);

  return (
    <div>
      <div className="mb-6">
        <h1 className="page-title">Seeding Dashboard</h1>
        <p className="page-subtitle">Monitor your seeding activity and earnings.</p>
      </div>

      {/* Network-wide stats — visible to everyone, no wallet required */}
      <div className="mb-8">
        <h2 className="text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-500 mb-3">
          Network Stats
        </h2>
        <div className="grid grid-cols-2 md:grid-cols-5 gap-4">
          <div className="card p-4">
            <p className="text-[10px] font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-500 mb-1">
              ARA Price
            </p>
            <p className="text-xl font-bold text-slate-900 dark:text-slate-100">
              {fmtUsdPrice(araPriceUsd)}
            </p>
            <a
              href="https://www.coingecko.com/en/coins/ara"
              target="_blank"
              rel="noreferrer"
              className="text-[10px] text-slate-500 hover:text-ara-500"
            >
              via CoinGecko
            </a>
          </div>
          <div className="card p-4">
            <p className="text-[10px] font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-500 mb-1">
              Total ARA Staked
            </p>
            <p className="text-xl font-bold text-slate-900 dark:text-slate-100">
              {overview ? fmtAra(overview.total_staked_ara) : "—"}
            </p>
            {overview && araPriceUsd > 0 && (
              <p className="text-[10px] text-slate-500">
                ≈ ${fmtAra(String(parseFloat(overview.total_staked_ara) * araPriceUsd))}
              </p>
            )}
          </div>
          {/*
            Volume cards — one per payment currency with non-zero activity. When only ETH
            has been used we show a single "Total Volume" card; once USDC (or any other
            supported ERC-20) content is purchased, additional cards appear automatically.
            The backend returns `volume_by_token` pre-formatted per token's decimals.
          */}
          {overview && overview.volume_by_token.length > 0 ? (
            overview.volume_by_token.map((tv) => (
              <div className="card p-4" key={tv.symbol + tv.address}>
                <p className="text-[10px] font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-500 mb-1">
                  Volume ({tv.symbol})
                </p>
                <p className="text-xl font-bold text-slate-900 dark:text-slate-100">
                  {fmtCrypto(tv.amount, tv.symbol)}
                </p>
              </div>
            ))
          ) : (
            <div className="card p-4">
              <p className="text-[10px] font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-500 mb-1">
                Total Volume
              </p>
              <p className="text-xl font-bold text-slate-900 dark:text-slate-100">
                {overview ? fmtCrypto(overview.total_volume_eth, "ETH") : "—"}
              </p>
            </div>
          )}
          <div className="card p-4">
            <p className="text-[10px] font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-500 mb-1">
              Seeder Rewards Paid
            </p>
            <p className="text-xl font-bold text-slate-900 dark:text-slate-100">
              {overview ? fmtCrypto(overview.total_rewards_paid_eth, "ETH") : "—"}
            </p>
          </div>
          <div className="card p-4">
            <p className="text-[10px] font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-500 mb-1">
              Content Published
            </p>
            <p className="text-xl font-bold text-slate-900 dark:text-slate-100">
              {overview ? overview.total_items : "—"}
            </p>
          </div>
        </div>
      </div>

      {/* Personal section header */}
      <h2 className="text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-500 mb-3 mt-8">
        My Seeding
      </h2>

      {!isConnected && (
        <div className="alert-warning mb-6">Connect your wallet to view your dashboard.</div>
      )}
      {error && <div className="alert-error mb-6">{error}</div>}

      {/* Personal stats cards */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-4 mb-8">
        {[
          { label: "Content Seeding", value: String(activeCount) },
          { label: "Data Served",     value: formatBytes(totalBytesServed) },
          { label: "Items Tracked",   value: String(stats.length) },
        ].map(({ label, value }) => (
          <div key={label} className="card p-6">
            <p className="text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-500 mb-2">
              {label}
            </p>
            <p className="text-2xl font-bold text-slate-900 dark:text-slate-100">{value}</p>
          </div>
        ))}
      </div>

      {loading ? (
        <div className="flex items-center justify-center py-16">
          <div className="w-8 h-8 border-2 border-ara-500 border-t-transparent rounded-full animate-spin" />
        </div>
      ) : stats.length === 0 ? (
        <div className="card p-10 text-center">
          <p className="text-slate-400 dark:text-slate-600 mb-1">No seeding activity</p>
          <p className="text-sm text-slate-500 dark:text-slate-600">
            Publish or purchase content, then start seeding to earn rewards.
          </p>
        </div>
      ) : (
        <div className="card overflow-hidden">
          <table className="w-full text-sm">
            <thead className="border-b border-slate-200 dark:border-slate-800">
              <tr>
                {["Content", "Bytes Served", "Peers", "Status"].map((h, i) => (
                  <th
                    key={h}
                    className={`px-4 py-3 text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-500 ${
                      i === 0 ? "text-left" : "text-right"
                    }`}
                  >
                    {h}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-100 dark:divide-slate-800/60">
              {stats.map((item) => (
                <tr key={item.content_id} className="hover:bg-slate-50 dark:hover:bg-slate-800/30 transition-colors">
                  <td className="px-4 py-3 text-slate-900 dark:text-slate-200 font-medium">
                    {item.title}
                  </td>
                  <td className="px-4 py-3 text-slate-500 dark:text-slate-400 text-right">
                    {formatBytes(item.bytes_served)}
                  </td>
                  <td className="px-4 py-3 text-slate-500 dark:text-slate-400 text-right">
                    {item.peer_count}
                  </td>
                  <td className="px-4 py-3 text-right">
                    <span className={item.is_active ? "badge-green" : "badge-gray"}>
                      {item.is_active ? "Active" : "Stopped"}
                    </span>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

export default Dashboard;
