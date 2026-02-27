import { useState, useEffect } from "react";
import { getDisplayName } from "../lib/tauri";

interface AddressDisplayProps {
  address: string;
  showFull?: boolean;
  className?: string;
}

const nameCache = new Map<string, string | null>();

export default function AddressDisplay({
  address,
  showFull,
  className,
}: AddressDisplayProps) {
  const [displayName, setDisplayName] = useState<string | null>(
    nameCache.get(address.toLowerCase()) ?? null
  );
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    const key = address.toLowerCase();
    if (nameCache.has(key)) {
      setDisplayName(nameCache.get(key)!);
      return;
    }
    getDisplayName(address)
      .then((name) => {
        nameCache.set(key, name);
        setDisplayName(name);
      })
      .catch(() => {});
  }, [address]);

  const truncated = `${address.slice(0, 6)}...${address.slice(-4)}`;
  const label = displayName || (showFull ? address : truncated);

  const handleCopy = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    navigator.clipboard.writeText(address);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  return (
    <span
      className={`inline-flex items-center gap-1 cursor-pointer group ${className ?? ""}`}
      title={`${address}${displayName ? ` (${displayName})` : ""}\nClick to copy`}
      onClick={handleCopy}
    >
      <span className="font-mono text-sm group-hover:text-ara-500 transition-colors">
        {label}
      </span>
      {copied && (
        <span className="text-xs text-green-500 ml-1">Copied!</span>
      )}
    </span>
  );
}

export function clearNameCache() {
  nameCache.clear();
}
