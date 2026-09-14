import { useState } from "react";
import { PieChart, Pie, Cell, Tooltip, ResponsiveContainer } from "recharts";
import { PieChart as PieIcon, BarChart2 } from "lucide-react";
import { useTranslation } from "react-i18next";

export type VersionDistributionItem = {
  name: string;
  count: number;
  percentage: number;
};

const PALETTE = [
  "#c8eca4",
  "#7ec8c4",
  "#e0b46a",
  "#d98b7a",
  "#c4b08a",
  "#8fb4c8",
  "#9fd47a",
  "#4aa8a4",
  "#e07a6a",
];

export function DonutChart({
  items,
  title,
  badge = "版本",
  height = 200,
  centerLabel,
}: {
  items: VersionDistributionItem[];
  title?: string;
  badge?: string;
  height?: number | string;
  centerLabel?: string;
}) {
  const { t } = useTranslation();
  const [viewMode, setViewMode] = useState<"donut" | "bar">("donut");
  const total = items.reduce((acc, curr) => acc + curr.count, 0);

  if (!items || items.length === 0) {
    return (
      <div className="w-full flex items-center justify-center text-xs text-[var(--muted)]" style={{ height: typeof height === "number" ? `${height}px` : height }}>
        {t("apps.noVersions")}
      </div>
    );
  }

  const chartData = items.slice(0, 8).map((item, idx) => {
    const computedPercentage = total > 0 ? Math.round((item.count / total) * 1000) / 10 : 0;
    return {
      name: item.name,
      count: item.count,
      percentage: item.percentage > 0 ? item.percentage : computedPercentage,
      color: PALETTE[idx % PALETTE.length],
    };
  });

  return (
    <div className="flex flex-col justify-between h-full min-h-[280px]">
      {/* Header */}
      <div className="flex items-center justify-between mb-2">
        <div className="flex items-center gap-2">
          <h3 className="text-sm font-bold text-[var(--text)] m-0">{title || t("apps.statsAppVersions")}</h3>
          {badge ? (
            <span className="rounded-md border border-[var(--amber)]/30 bg-[var(--amber-subtle)] px-1.5 py-0.5 text-[10px] font-bold text-[var(--amber)]">
              {badge}
            </span>
          ) : null}
        </div>

        {/* View Toggle */}
        <div className="inline-flex rounded-lg border border-[var(--border)] bg-[var(--input-bg)] p-0.5">
          <button
            type="button"
            className={`flex items-center gap-1 px-2 py-0.5 rounded-md text-[11px] font-semibold transition-all cursor-pointer ${
              viewMode === "donut" ? "bg-[var(--panel-strong)] text-[var(--text)] shadow-sm" : "text-[var(--muted)] hover:text-[var(--text)]"
            }`}
            onClick={() => setViewMode("donut")}
            title={t("stats.donutView")}
          >
            <PieIcon size={12} />
            <span>{t("stats.donutView")}</span>
          </button>
          <button
            type="button"
            className={`flex items-center gap-1 px-2 py-0.5 rounded-md text-[11px] font-semibold transition-all cursor-pointer ${
              viewMode === "bar" ? "bg-[var(--panel-strong)] text-[var(--text)] shadow-sm" : "text-[var(--muted)] hover:text-[var(--text)]"
            }`}
            onClick={() => setViewMode("bar")}
            title={t("stats.barView")}
          >
            <BarChart2 size={12} />
            <span>{t("stats.barView")}</span>
          </button>
        </div>
      </div>

      {viewMode === "donut" ? (
        <div className="flex-1 flex flex-col items-center justify-center min-h-[160px]">
          <div className="relative w-full flex-1 min-h-[140px]">
            <ResponsiveContainer width="100%" height="100%">
              <PieChart>
                <Pie
                  data={chartData}
                  cx="50%"
                  cy="50%"
                  innerRadius="62%"
                  outerRadius="84%"
                  paddingAngle={3}
                  dataKey="count"
                >
                  {chartData.map((entry, index) => (
                    <Cell key={`cell-${index}`} fill={entry.color} stroke="transparent" />
                  ))}
                </Pie>
                <Tooltip
                  content={({ active, payload }) => {
                    if (active && payload && payload.length) {
                      const data = payload[0].payload;
                      return (
                        <div className="rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--bg-float)] p-2.5 shadow-xl text-xs space-y-1">
                          <p className="font-bold text-[var(--text)]">{data.name}</p>
                          <p style={{ color: data.color }} className="font-mono">
                            {data.count.toLocaleString()} ({data.percentage}%)
                          </p>
                        </div>
                      );
                    }
                    return null;
                  }}
                />
              </PieChart>
            </ResponsiveContainer>

            {/* Center Summary */}
            <div className="pointer-events-none absolute inset-0 flex flex-col items-center justify-center text-center">
              <span className="text-[10px] text-[var(--muted)]">{centerLabel || t("apps.statsTotalEvents")}</span>
              <strong className="text-base font-extrabold font-mono text-[var(--text)]">
                {total.toLocaleString()}
              </strong>
            </div>
          </div>

          {/* Bottom Legend */}
          <div className="flex flex-wrap items-center justify-center gap-1.5 mt-2 max-h-16 overflow-y-auto w-full">
            {chartData.map((item) => (
              <span
                key={item.name}
                className="inline-flex items-center gap-1.5 px-2 py-0.5 rounded-md bg-[var(--input-bg)] border border-[var(--border-soft)] text-[10px] text-[var(--muted)]"
              >
                <span className="h-2 w-2 rounded-full" style={{ backgroundColor: item.color }} />
                <strong className="text-[var(--text)] max-w-[80px] truncate" title={item.name}>
                  {item.name}
                </strong>
                <span className="font-mono">{item.percentage}%</span>
              </span>
            ))}
          </div>
        </div>
      ) : (
        <div className="space-y-2 max-h-48 overflow-y-auto pr-1 flex-1">
          {items.map((item, idx) => {
            const color = PALETTE[idx % PALETTE.length];
            const pct = item.percentage > 0 ? item.percentage : (total > 0 ? Math.round((item.count / total) * 1000) / 10 : 0);
            return (
              <div key={item.name} className="flex items-center gap-3 text-xs">
                <span className="w-24 truncate font-medium text-[var(--text)]" title={item.name}>
                  {item.name}
                </span>
                <div className="flex-1 h-2 rounded-full bg-[var(--input-bg)] overflow-hidden">
                  <div
                    className="h-full rounded-full transition-all duration-300"
                    style={{ width: `${pct}%`, backgroundColor: color }}
                  />
                </div>
                <span className="w-12 text-right font-mono text-[var(--muted)]">{item.count}</span>
                <span className="w-12 text-right font-mono font-bold" style={{ color }}>
                  {pct}%
                </span>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
