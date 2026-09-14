import { useState, useMemo } from "react";
import { useTranslation } from "react-i18next";
import {
  BarChart,
  Bar,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
  Cell,
} from "recharts";
import {
  PlatformIcon,
  getPlatformFamily,
  getPlatformColor,
  type PlatformFamily,
} from "./PlatformIcon";

export type BuildItem = {
  name: string;
  count: number;
  percentage: number;
};

export function BuildBarChart({
  items,
  height = "100%",
  badge = "Build",
}: {
  items: BuildItem[];
  height?: number | string;
  badge?: string;
}) {
  const { t } = useTranslation();
  const [activeFamily, setActiveFamily] = useState<PlatformFamily | "all">("all");

  // Group items by platform family
  const { windowsItems, linuxItems, macosItems, mobileItems, hasWindows, hasLinux, hasMac, hasMobile } =
    useMemo(() => {
      const win: BuildItem[] = [];
      const lin: BuildItem[] = [];
      const mac: BuildItem[] = [];
      const mob: BuildItem[] = [];

      for (const item of items ?? []) {
        const fam = getPlatformFamily(item.name);
        if (fam === "windows") win.push(item);
        else if (fam === "linux") lin.push(item);
        else if (fam === "macos") mac.push(item);
        else if (fam === "mobile") mob.push(item);
        else lin.push(item);
      }

      return {
        windowsItems: win,
        linuxItems: lin,
        macosItems: mac,
        mobileItems: mob,
        hasWindows: win.length > 0,
        hasLinux: lin.length > 0,
        hasMac: mac.length > 0,
        hasMobile: mob.length > 0,
      };
    }, [items]);

  // Compute items to display
  const chartData = useMemo(() => {
    if (!items || items.length === 0) return [];

    let targetItems: BuildItem[] = [];

    if (activeFamily === "windows") {
      targetItems = windowsItems.slice(0, 10);
    } else if (activeFamily === "linux") {
      targetItems = linuxItems.slice(0, 10);
    } else if (activeFamily === "macos") {
      targetItems = macosItems.slice(0, 10);
    } else if (activeFamily === "mobile") {
      targetItems = mobileItems.slice(0, 10);
    } else {
      // In "all" mode, ensure cross-platform representation
      if (hasWindows && hasLinux) {
        const topWin = windowsItems.slice(0, 5);
        const topLin = linuxItems.slice(0, 5);
        const topOther = [...macosItems, ...mobileItems].slice(0, 2);
        targetItems = [...topWin, ...topLin, ...topOther];
      } else {
        targetItems = items.slice(0, 10);
      }
    }

    return targetItems.map((b) => ({
      name: b.name,
      displayName: b.name.replace(/^Win\s*/i, "Win ").replace(/^Linux\s*/i, ""),
      count: b.count,
      percentage: b.percentage,
      fill: getPlatformColor(b.name),
      family: getPlatformFamily(b.name),
    }));
  }, [items, activeFamily, windowsItems, linuxItems, macosItems, mobileItems, hasWindows, hasLinux]);

  if (!items || items.length === 0) {
    return (
      <div
        className="w-full flex items-center justify-center text-xs text-[var(--muted)]"
        style={{ height: typeof height === "number" ? `${height}px` : height }}
      >
        {t("apps.noOS")}
      </div>
    );
  }

  return (
    <div
      className="w-full h-full min-h-[220px] flex flex-col justify-between"
      style={{ height: typeof height === "number" ? `${height}px` : height }}
    >
      {/* Platform Filter Pills Header (Strictly single-line with flex-nowrap & horizontal scroll) */}
      <div className="flex items-center gap-2 overflow-x-auto pb-1.5 mb-2 scrollbar-none flex-nowrap">
        <button
          type="button"
          className={`px-3 py-1 rounded-[var(--radius-sm)] text-xs font-semibold transition-colors cursor-pointer inline-flex items-center gap-1.5 whitespace-nowrap flex-shrink-0 ${
            activeFamily === "all"
              ? "bg-[var(--signal)] text-[var(--signal-ink)] font-bold"
              : "bg-[var(--input-bg)] text-[var(--muted)] hover:text-[var(--text)] border border-[var(--border-soft)]"
          }`}
          onClick={() => setActiveFamily("all")}
        >
          <span>{t("builds.allSystems")}</span>
          <span className="opacity-75 font-mono text-[10px]">({items.length})</span>
        </button>

        {hasWindows ? (
          <button
            type="button"
            className={`px-3 py-1 rounded-[var(--radius-sm)] text-xs font-semibold transition-colors cursor-pointer inline-flex items-center gap-1.5 whitespace-nowrap flex-shrink-0 ${
              activeFamily === "windows"
                ? "bg-[var(--blue)] text-[var(--signal-ink)] font-bold"
                : "bg-[var(--input-bg)] text-[var(--muted)] hover:text-[var(--text)] border border-[var(--border-soft)]"
            }`}
            onClick={() => setActiveFamily(activeFamily === "windows" ? "all" : "windows")}
          >
            <PlatformIcon platform="windows" size={12} />
            <span>Windows</span>
            <span className="opacity-75 font-mono text-[10px]">({windowsItems.length})</span>
          </button>
        ) : null}

        {hasLinux ? (
          <button
            type="button"
            className={`px-3 py-1 rounded-[var(--radius-sm)] text-xs font-semibold transition-colors cursor-pointer inline-flex items-center gap-1.5 whitespace-nowrap flex-shrink-0 ${
              activeFamily === "linux"
                ? "bg-[var(--signal)] text-[var(--signal-ink)] font-bold"
                : "bg-[var(--input-bg)] text-[var(--muted)] hover:text-[var(--text)] border border-[var(--border-soft)]"
            }`}
            onClick={() => setActiveFamily(activeFamily === "linux" ? "all" : "linux")}
          >
            <PlatformIcon platform="linux" size={12} />
            <span>Linux</span>
            <span className="opacity-75 font-mono text-[10px]">({linuxItems.length})</span>
          </button>
        ) : null}

        {hasMac ? (
          <button
            type="button"
            className={`px-3 py-1 rounded-[var(--radius-sm)] text-xs font-semibold transition-colors cursor-pointer inline-flex items-center gap-1.5 whitespace-nowrap flex-shrink-0 ${
              activeFamily === "macos"
                ? "bg-[var(--purple)] text-[var(--signal-ink)] font-bold"
                : "bg-[var(--input-bg)] text-[var(--muted)] hover:text-[var(--text)] border border-[var(--border-soft)]"
            }`}
            onClick={() => setActiveFamily(activeFamily === "macos" ? "all" : "macos")}
          >
            <PlatformIcon platform="macos" size={12} />
            <span>macOS</span>
            <span className="opacity-75 font-mono text-[10px]">({macosItems.length})</span>
          </button>
        ) : null}

        {hasMobile ? (
          <button
            type="button"
            className={`px-3 py-1 rounded-[var(--radius-sm)] text-xs font-semibold transition-colors cursor-pointer inline-flex items-center gap-1.5 whitespace-nowrap flex-shrink-0 ${
              activeFamily === "mobile"
                ? "bg-[var(--danger)] text-white font-bold"
                : "bg-[var(--input-bg)] text-[var(--muted)] hover:text-[var(--text)] border border-[var(--border-soft)]"
            }`}
            onClick={() => setActiveFamily(activeFamily === "mobile" ? "all" : "mobile")}
          >
            <PlatformIcon platform="mobile" size={12} />
            <span>Mobile</span>
            <span className="opacity-75 font-mono text-[10px]">({mobileItems.length})</span>
          </button>
        ) : null}
      </div>

      {/* Bar Chart Area with Non-Clipped Ticks */}
      <div className="flex-1 w-full relative min-h-[160px]">
        <ResponsiveContainer width="100%" height="100%">
          <BarChart
            data={chartData}
            margin={{ top: 12, right: 16, left: -10, bottom: 40 }}
          >
            <CartesianGrid strokeDasharray="3 3" stroke="var(--border-soft)" vertical={false} />
            <XAxis
              dataKey="displayName"
              stroke="var(--muted)"
              fontSize={10}
              tickLine={false}
              axisLine={false}
              fontFamily="var(--font-mono)"
              interval={0}
              angle={-22}
              textAnchor="end"
              height={44}
              dx={-4}
              dy={6}
            />
            <YAxis
              stroke="var(--muted)"
              fontSize={10}
              tickLine={false}
              axisLine={false}
              fontFamily="var(--font-mono)"
              width={38}
              allowDecimals={false}
              tickFormatter={(v: number) => (v >= 1000 ? `${(v / 1000).toFixed(0)}k` : String(v))}
            />
            <Tooltip
              content={({ active, payload }) => {
                if (active && payload && payload.length) {
                  const item = payload[0].payload;
                  return (
                    <div className="rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--bg-float)] p-3 shadow-xl text-xs space-y-1.5 min-w-[150px]">
                      <div className="flex items-center gap-1.5 font-bold text-[var(--text)]">
                        <PlatformIcon platform={item.name} size={14} />
                        <span>{item.name}</span>
                      </div>
                      <div className="flex items-center justify-between gap-3 font-mono">
                        <span className="text-[var(--muted)]">{t("overview.count")}:</span>
                        <strong style={{ color: item.fill }}>{item.count?.toLocaleString()}</strong>
                      </div>
                      <div className="flex items-center justify-between gap-3 font-mono text-[10px] text-[var(--muted)]">
                        <span>{t("builds.share")}</span>
                        <span>{item.percentage}%</span>
                      </div>
                    </div>
                  );
                }
                return null;
              }}
            />
            <Bar dataKey="count" radius={[6, 6, 0, 0]} maxBarSize={36}>
              {chartData.map((entry, index) => (
                <Cell key={`cell-${index}`} fill={entry.fill} />
              ))}
            </Bar>
          </BarChart>
        </ResponsiveContainer>
      </div>
    </div>
  );
}
