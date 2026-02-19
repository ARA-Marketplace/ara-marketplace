import { useState, useEffect } from "react";
import { getSeederStats, type SeederStats } from "../lib/tauri";
import { useWeb3ModalAccount } from "@web3modal/ethers/react";

function Dashboard() {
  const { isConnected } = useWeb3ModalAccount();
  const [stats, setStats] = useState<SeederStats[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!isConnected) {
      setStats([]);
      setLoading(false);
      return;
    }

    const fetchStats = () => {
      getSeederStats()
        .then(setStats)
        .catch((e) => setError(String(e)))
        .finally(() => setLoading(false));
    };

    fetchStats();
    const interval = setInterval(fetchStats, 10000);
    return () => clearInterval(interval);
  }, [isConnected]);

  const activeCount = stats.filter((s) => s.is_active).length;
  const totalBytesServed = stats.reduce((sum, s) => sum + s.bytes_served, 0);

  const formatBytes = (bytes: number) => {
    if (bytes === 0) return "0 B";
    const units = ["B", "KB", "MB", "GB", "TB"];
    const i = Math.floor(Math.log(bytes) / Math.log(1024));
    return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${units[i]}`;
  };

  return (
    <div>
      <h1 className="text-3xl font-bold text-gray-900">Seeding Dashboard</h1>
      <p className="mt-2 text-gray-600 mb-8">
        Monitor your seeding activity and earnings.
      </p>

      {!isConnected && (
        <div className="p-4 bg-yellow-50 border border-yellow-200 rounded-lg text-yellow-700 text-sm mb-6">
          Connect your wallet to view your dashboard.
        </div>
      )}

      {error && (
        <div className="p-4 bg-red-50 border border-red-200 rounded-lg text-red-700 text-sm mb-6">
          {error}
        </div>
      )}

      <div className="grid grid-cols-1 md:grid-cols-3 gap-6 mb-8">
        <div className="bg-white rounded-lg shadow-sm border border-gray-200 p-6">
          <p className="text-sm text-gray-500">Content Seeding</p>
          <p className="text-2xl font-bold text-gray-900 mt-1">
            {activeCount}
          </p>
        </div>
        <div className="bg-white rounded-lg shadow-sm border border-gray-200 p-6">
          <p className="text-sm text-gray-500">Data Served</p>
          <p className="text-2xl font-bold text-gray-900 mt-1">
            {formatBytes(totalBytesServed)}
          </p>
        </div>
        <div className="bg-white rounded-lg shadow-sm border border-gray-200 p-6">
          <p className="text-sm text-gray-500">Total Items Tracked</p>
          <p className="text-2xl font-bold text-gray-900 mt-1">
            {stats.length}
          </p>
        </div>
      </div>

      {loading ? (
        <div className="text-center text-gray-400 py-8">Loading...</div>
      ) : stats.length === 0 ? (
        <div className="bg-white rounded-lg shadow-sm border border-gray-200 p-8 text-center text-gray-400">
          <p className="text-lg">No seeding activity</p>
          <p className="mt-2 text-sm">
            Publish or purchase content, then start seeding to earn rewards
          </p>
        </div>
      ) : (
        <div className="bg-white rounded-lg shadow-sm border border-gray-200 overflow-hidden">
          <table className="w-full">
            <thead className="bg-gray-50 border-b border-gray-200">
              <tr>
                <th className="text-left px-4 py-3 text-sm font-medium text-gray-500">
                  Content
                </th>
                <th className="text-right px-4 py-3 text-sm font-medium text-gray-500">
                  Bytes Served
                </th>
                <th className="text-right px-4 py-3 text-sm font-medium text-gray-500">
                  Peers
                </th>
                <th className="text-right px-4 py-3 text-sm font-medium text-gray-500">
                  Status
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-100">
              {stats.map((item) => (
                <tr key={item.content_id}>
                  <td className="px-4 py-3 text-sm text-gray-900">
                    {item.title}
                  </td>
                  <td className="px-4 py-3 text-sm text-gray-600 text-right">
                    {formatBytes(item.bytes_served)}
                  </td>
                  <td className="px-4 py-3 text-sm text-gray-600 text-right">
                    {item.peer_count}
                  </td>
                  <td className="px-4 py-3 text-right">
                    <span
                      className={`text-xs px-2 py-1 rounded-full ${
                        item.is_active
                          ? "bg-green-100 text-green-700"
                          : "bg-gray-100 text-gray-500"
                      }`}
                    >
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
