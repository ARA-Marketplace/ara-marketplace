import type { PricePoint } from "../lib/tauri";

interface PriceHistoryChartProps {
  data: PricePoint[];
  className?: string;
}

export default function PriceHistoryChart({
  data,
  className,
}: PriceHistoryChartProps) {
  if (data.length === 0) {
    return (
      <div className={`flex items-center justify-center h-40 text-gray-400 dark:text-gray-500 text-sm ${className ?? ""}`}>
        No sales yet
      </div>
    );
  }

  const prices = data.map((p) => parseFloat(p.price_eth));
  const minPrice = Math.min(...prices);
  const maxPrice = Math.max(...prices);
  const range = maxPrice - minPrice || 1;

  const width = 400;
  const height = 160;
  const padding = { top: 20, right: 40, bottom: 30, left: 10 };
  const plotW = width - padding.left - padding.right;
  const plotH = height - padding.top - padding.bottom;

  const points = data.map((p, i) => {
    const x = padding.left + (data.length === 1 ? plotW / 2 : (i / (data.length - 1)) * plotW);
    const y = padding.top + plotH - ((parseFloat(p.price_eth) - minPrice) / range) * plotH;
    return { x, y, price: p.price_eth, isResale: p.is_resale };
  });

  const pathD = points
    .map((p, i) => `${i === 0 ? "M" : "L"} ${p.x} ${p.y}`)
    .join(" ");

  // Y-axis labels
  const yLabels = [maxPrice, (maxPrice + minPrice) / 2, minPrice].map((v) => ({
    value: v.toFixed(v < 0.01 ? 6 : 4),
    y: padding.top + plotH - ((v - minPrice) / range) * plotH,
  }));

  return (
    <div className={className}>
      <svg viewBox={`0 0 ${width} ${height}`} className="w-full h-auto">
        {/* Grid lines */}
        {yLabels.map((l, i) => (
          <g key={i}>
            <line
              x1={padding.left}
              y1={l.y}
              x2={width - padding.right}
              y2={l.y}
              stroke="currentColor"
              strokeOpacity={0.1}
              strokeDasharray="4 2"
            />
            <text
              x={width - padding.right + 4}
              y={l.y + 4}
              className="text-[9px] fill-gray-400 dark:fill-gray-500"
            >
              {l.value}
            </text>
          </g>
        ))}

        {/* Line */}
        <path
          d={pathD}
          fill="none"
          stroke="#7c3aed"
          strokeWidth={2}
          strokeLinejoin="round"
        />

        {/* Area fill */}
        {points.length > 1 && (
          <path
            d={`${pathD} L ${points[points.length - 1].x} ${padding.top + plotH} L ${points[0].x} ${padding.top + plotH} Z`}
            fill="url(#areaGrad)"
            opacity={0.15}
          />
        )}

        {/* Points */}
        {points.map((p, i) => (
          <circle
            key={i}
            cx={p.x}
            cy={p.y}
            r={3}
            fill={p.isResale ? "#f59e0b" : "#7c3aed"}
            stroke="white"
            strokeWidth={1}
          >
            <title>
              {p.price} ETH{p.isResale ? " (resale)" : ""}
            </title>
          </circle>
        ))}

        <defs>
          <linearGradient id="areaGrad" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor="#7c3aed" />
            <stop offset="100%" stopColor="#7c3aed" stopOpacity={0} />
          </linearGradient>
        </defs>

        {/* ETH label */}
        <text
          x={width - padding.right + 4}
          y={padding.top - 6}
          className="text-[8px] fill-gray-400 dark:fill-gray-500"
        >
          ETH
        </text>
      </svg>
      {data.length > 0 && (
        <div className="flex gap-4 text-xs text-gray-400 dark:text-gray-500 mt-1">
          <span className="flex items-center gap-1">
            <span className="w-2 h-2 rounded-full bg-purple-600 inline-block" />
            Primary
          </span>
          <span className="flex items-center gap-1">
            <span className="w-2 h-2 rounded-full bg-amber-500 inline-block" />
            Resale
          </span>
        </div>
      )}
    </div>
  );
}
