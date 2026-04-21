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
              {araPriceUsd > 0 ? `$${araPriceUsd.toFixed(4)}` : "—"}
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
          <div className="card p-4">
            <p className="text-[10px] font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-500 mb-1">
              Total Volume
            </p>
            <p className="text-xl font-bold text-slate-900 dark:text-slate-100">
              {overview ? `${parseFloat(overview.total_volume_eth).toFixed(3)} ETH` : "—"}
            </p>
          </div>
          <div className="card p-4">
            <p className="text-[10px] font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-500 mb-1">
              Seeder Rewards Paid
            </p>
            <p className="text-xl font-bold text-slate-900 dark:text-slate-100">
              {overview ? `${parseFloat(overview.total_rewards_paid_eth).toFixed(3)} ETH` : "—"}
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

      {!isConnected && (
        <div className="alert-warning mb-6">Connect your wallet to view your dashboard.</div>
      )}
      {error && <div className="alert-error mb-6">{error}</div>}

      {/* Stats cards */}
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
