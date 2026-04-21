import { useEffect, useState, useMemo } from "react";
import { Link, useParams } from "react-router-dom";
import {
  getCreatorContent,
  getDisplayName,
  getPreviewAsset,
  tipContent,
  confirmTip,
  type ContentDetail,
} from "../lib/tauri";
import { useWeb3Modal, useWeb3ModalProvider, useWeb3ModalAccount } from "@web3modal/ethers/react";
import { signAndSendTransactions } from "../lib/transactions";
import { convertFileSrc } from "@tauri-apps/api/core";
import AddressDisplay from "../components/AddressDisplay";

export default function CreatorProfile() {
  const { address } = useParams<{ address: string }>();
  const [items, setItems] = useState<ContentDetail[]>([]);
  const [displayName, setDisplayName] = useState<string | null>(null);
  const [previewSrcs, setPreviewSrcs] = useState<Record<string, string>>({});
  const [loading, setLoading] = useState(true);

  // Tip state — tips the most recently published content by default
  const { open: openModal } = useWeb3Modal();
  const { isConnected, address: viewerAddress } = useWeb3ModalAccount();
  const { walletProvider } = useWeb3ModalProvider();
  const [tipAmount, setTipAmount] = useState("");
  const [tipTargetId, setTipTargetId] = useState<string>("");
  const [tipStep, setTipStep] = useState<"idle" | "signing" | "confirming" | "done">("idle");
  const [tipError, setTipError] = useState<string | null>(null);

  useEffect(() => {
    if (!address) return;
    setLoading(true);
    Promise.all([getCreatorContent(address), getDisplayName(address).catch(() => null)])
      .then(([content, name]) => {
        setItems(content);
        setDisplayName(name);
        if (content.length > 0) setTipTargetId(content[0].content_id);
      })
      .catch(() => {})
      .finally(() => setLoading(false));
  }, [address]);

  // Load preview images lazily
  useEffect(() => {
    items.forEach((item) => {
      if (previewSrcs[item.content_id]) return;
      try {
        const meta = JSON.parse(item.metadata_uri);
        if (!meta.main_preview_image?.hash) return;
        getPreviewAsset({
          contentId: item.content_id,
          previewHash: meta.main_preview_image.hash,
          filename: meta.main_preview_image.filename,
        })
          .then((path) => convertFileSrc(path, "localasset"))
          .then((src) => setPreviewSrcs((prev) => ({ ...prev, [item.content_id]: src })))
          .catch(() => {});
      } catch {}
    });
  }, [items]);

  const totalListVolume = useMemo(
    () => items.reduce((sum, it) => sum + parseFloat(it.price_eth || "0"), 0),
    [items],
  );

  const isSelf = viewerAddress && address && viewerAddress.toLowerCase() === address.toLowerCase();

  const handleTip = async () => {
    if (!address || !tipTargetId || !tipAmount.trim()) return;
    if (!isConnected) { openModal(); return; }
    if (!walletProvider) return;
    const amt = parseFloat(tipAmount);
    if (isNaN(amt) || amt <= 0) { setTipError("Enter a valid tip amount"); return; }
    setTipError(null);
    setTipStep("signing");
    try {
      const txs = await tipContent({ contentId: tipTargetId, tipAmountEth: tipAmount });
      const txHash = await signAndSendTransactions(walletProvider, txs, () => {});
      setTipStep("confirming");
      await confirmTip({ contentId: tipTargetId, txHash, tipAmountEth: tipAmount });
      setTipStep("done");
      setTipAmount("");
      setTimeout(() => setTipStep("idle"), 3000);
    } catch (e: unknown) {
      setTipError(e instanceof Error ? e.message : String(e));
      setTipStep("idle");
    }
  };

  if (!address) return <div className="card p-10 text-center">Invalid creator address.</div>;

  return (
    <div>
      <div className="mb-6">
        <Link to="/creators" className="text-sm text-ara-500 hover:text-ara-400">
          ← Back to Top Creators
        </Link>
      </div>

      {/* Creator header */}
      <div className="card p-6 mb-6 bg-gradient-to-br from-ara-700/10 via-purple-700/10 to-indigo-800/10">
        <div className="flex flex-col md:flex-row md:items-center md:justify-between gap-4">
          <div>
            <h1 className="text-2xl font-bold dark:text-white">
              {displayName ?? <AddressDisplay address={address} />}
            </h1>
            {displayName && (
              <p className="text-xs font-mono text-slate-500 dark:text-slate-500 mt-1">
                {address}
              </p>
            )}
            <div className="flex gap-6 mt-3 text-sm text-slate-600 dark:text-slate-400">
              <span><span className="font-semibold">{items.length}</span> items published</span>
              <span><span className="font-semibold">{totalListVolume.toFixed(4)} ETH</span> list volume</span>
            </div>
          </div>

          {!isSelf && items.length > 0 && (
            <div className="card p-4 min-w-[280px]">
              <p className="text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-500 mb-2">
                Tip Creator
              </p>
              <select
                value={tipTargetId}
                onChange={(e) => setTipTargetId(e.target.value)}
                disabled={tipStep !== "idle"}
                className="input-base text-xs py-1.5 mb-2"
              >
                {items.map((it) => (
                  <option key={it.content_id} value={it.content_id}>
                    {it.title || "(untitled)"}
                  </option>
                ))}
              </select>
              <div className="flex gap-2">
                <input
                  type="text"
                  value={tipAmount}
                  onChange={(e) => setTipAmount(e.target.value)}
                  placeholder="0.01"
                  disabled={tipStep !== "idle"}
                  className="input-base flex-1 text-sm"
                />
                <button
                  onClick={handleTip}
                  disabled={tipStep !== "idle" || !tipAmount.trim()}
                  className="btn-secondary text-sm px-3 whitespace-nowrap"
                >
                  {tipStep === "idle" ? "Tip ETH" : tipStep === "done" ? "Sent!" : "Sending…"}
                </button>
              </div>
              {tipError && <p className="text-[10px] text-red-500 mt-1">{tipError}</p>}
              <p className="text-[10px] text-slate-400 dark:text-slate-600 mt-1.5">
                Tips split 85% creator / 2.5% stakers / 12.5% seeders
              </p>
            </div>
          )}
        </div>
      </div>

      {/* Content grid */}
      {loading ? (
        <div className="flex items-center justify-center py-16">
          <div className="w-8 h-8 border-2 border-ara-500 border-t-transparent rounded-full animate-spin" />
        </div>
      ) : items.length === 0 ? (
        <div className="card p-10 text-center text-slate-500 dark:text-slate-500">
          This creator hasn't published any active content yet.
        </div>
      ) : (
        <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-4">
          {items.map((item) => (
            <Link
              key={item.content_id}
              to={`/content/${encodeURIComponent(item.content_id)}`}
              className="card card-hover overflow-hidden group"
            >
              <div className="aspect-square bg-gradient-to-br from-ara-700/40 via-purple-700/40 to-indigo-800/40 relative overflow-hidden">
                {previewSrcs[item.content_id] && (
                  <img
                    src={previewSrcs[item.content_id]}
                    alt={item.title}
                    className="w-full h-full object-cover group-hover:scale-[1.03] transition-transform duration-300"
                  />
                )}
              </div>
              <div className="p-3">
                <h3 className="font-semibold text-sm truncate dark:text-white">{item.title || "(untitled)"}</h3>
                <div className="flex items-center justify-between mt-2">
                  {parseFloat(item.price_eth) === 0 ? (
                    <span className="text-green-500 font-bold text-sm">Free</span>
                  ) : (
                    <span className="text-ara-600 dark:text-ara-400 font-bold text-sm">
                      {item.price_eth} {item.payment_token_symbol ?? "ETH"}
                    </span>
                  )}
                  <span className="text-xs text-slate-400">{item.content_type}</span>
                </div>
              </div>
            </Link>
          ))}
        </div>
      )}
    </div>
  );
}
