import { useTranslation } from "react-i18next";
import {
  AreaChart,
  Area,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
} from "recharts";
import { shortDayLabel, compactNumber, ACTIVE_DOT_HALO } from "./chartTheme";

export type SplinePoint = {
  label: string;
  value: number;
  secondaryValue?: number;
};

export function SplineAreaChart({
  data,
  height = 180,
  strokeColor = "var(--signal)",
  fillColor = "var(--signal)",
  valueLabel = "Events",
  secondaryLabel = "Users",
}: {
  data: SplinePoint[];
  height?: number | string;
  strokeColor?: string;
  fillColor?: string;
  valueLabel?: string;
  secondaryLabel?: string;
}) {
  const { t } = useTranslation();

  if (!data || data.length === 0) {
    return (
      <div
        className="w-full flex items-center justify-center text-xs text-[var(--muted)]"
        style={{ height: typeof height === "number" ? `${height}px` : height }}
      >
        {t("explorer.empty")}
      </div>
    );
  }

  const chartData = data.map((d) => ({
    name: shortDayLabel(d.label),
    fullName: d.label,
    value: d.value,
    secondaryValue: d.secondaryValue,
  }));

  const gradientId = "areaGradient_" + Math.random().toString(36).substring(2, 9);

  return (
    <div className="w-full h-full min-h-[160px] relative" style={{ height: typeof height === "number" ? `${height}px` : height }}>
      <ResponsiveContainer width="100%" height="100%">
        <AreaChart data={chartData} margin={{ top: 10, right: 12, left: -16, bottom: 0 }}>
          <defs>
            <linearGradient id={gradientId} x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stopColor={fillColor} stopOpacity={0.35} />
              <stop offset="60%" stopColor={fillColor} stopOpacity={0.08} />
              <stop offset="100%" stopColor={fillColor} stopOpacity={0} />
            </linearGradient>
          </defs>
          <CartesianGrid strokeDasharray="3 3" stroke="var(--border-soft)" vertical={false} />
          <XAxis
            dataKey="name"
            stroke="var(--muted)"
            fontSize={10}
            tickLine={false}
            axisLine={false}
            fontFamily="var(--font-mono)"
            minTickGap={24}
          />
          <YAxis
            stroke="var(--faint)"
            fontSize={10}
            tickLine={false}
            axisLine={false}
            fontFamily="var(--font-mono)"
            width={38}
            tickFormatter={(v: number) => compactNumber(v)}
            allowDecimals={false}
          />
          <Tooltip
            cursor={{ stroke: "var(--border)", strokeWidth: 1 }}
            content={({ active, payload }) => {
              if (active && payload && payload.length) {
                const item = payload[0].payload;
                return (
                  <div className="rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--bg-float)] p-2.5 shadow-xl text-xs space-y-1">
                    <p className="font-bold text-[var(--text)]">{item.fullName}</p>
                    <p style={{ color: strokeColor }} className="font-mono">
                      {valueLabel}: <strong>{item.value?.toLocaleString()}</strong>
                    </p>
                    {item.secondaryValue !== undefined ? (
                      <p className="text-[var(--amber)] font-mono text-[11px]">
                        {secondaryLabel}: <strong>{item.secondaryValue?.toLocaleString()}</strong>
                      </p>
                    ) : null}
                  </div>
                );
              }
              return null;
            }}
          />
          <Area
            type="monotone"
            dataKey="value"
            stroke={strokeColor}
            strokeWidth={2}
            fillOpacity={1}
            fill={`url(#${gradientId})`}
            activeDot={{ r: 4, strokeWidth: 2, stroke: ACTIVE_DOT_HALO, fill: strokeColor }}
          />
        </AreaChart>
      </ResponsiveContainer>
    </div>
  );
}
