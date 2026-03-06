/**
 * Shared formatting utilities used across multiple pages/components.
 */

/** Format a byte count into a human-readable string (e.g. "1.4 MB"). Returns "—" for zero/negative values. */
export function fmtBytes(bytes: number): string {
  if (bytes <= 0) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${units[i]}`;
}

/** Format a Unix timestamp (seconds) to a locale date string (e.g. "3/6/2026"). */
export function fmtDate(ts: number): string {
  return new Date(ts * 1000).toLocaleDateString();
}

/** Emoji icon map keyed by content type. Use `TYPE_ICONS[type] ?? "📦"` for a safe fallback. */
export const TYPE_ICONS: Record<string, string> = {
  game: "🎮",
  music: "🎵",
  video: "🎬",
  document: "📄",
  software: "💾",
  other: "📦",
};
