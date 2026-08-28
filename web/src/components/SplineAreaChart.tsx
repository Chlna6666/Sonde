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

export type SplinePoint = {
  label: string;
  value: number;
  secondaryValue?: number;
};

export function SplineAreaChart({
  data,
  height = 180,
  strokeColor = "#22c55e",
  fillColor = "#22c55e",
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
    name: d.label.length > 10 ? d.label.slice(11) : d.label.length > 5 ? d.label.slice(5) : d.label,
    fullName: d.label,
    value: d.value,
    secondaryValue: d.secondaryValue,
  }));

  const gradientId = "areaGradient_" + Math.random().toString(36).substring(2, 9);

  return (
    <div className="w-full h-full min-h-[160px] relative" style={{ height: typeof height === "number" ? `${height}px` : height }}>
      <ResponsiveContainer width="100%" height="100%">
        <AreaChart data={chartData} margin={{ top: 10, right: 10, left: -20, bottom: 0 }}>
          <defs>
            <linearGradient id={gradientId} x1="0" y1="0" x2="0" y2="1">
              <stop offset="5%" stopColor={fillColor} stopOpacity={0.4} />
              <stop offset="65%" stopColor={fillColor} stopOpacity={0.08} />
              <stop offset="95%" stopColor={fillColor} stopOpacity={0} />
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
          />
          <YAxis
            stroke="var(--faint)"
            fontSize={10}
            tickLine={false}
            axisLine={false}
            fontFamily="var(--font-mono)"
            width={34}
          />
          <Tooltip
            content={({ active, payload }) => {
              if (active && payload && payload.length) {
                const item = payload[0].payload;
                return (
                  <div className="rounded-xl border border-[var(--border)] bg-[var(--panel-strong)] p-2.5 shadow-xl backdrop-blur-xl text-xs space-y-1">
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
            strokeWidth={2.5}
            fillOpacity={1}
            fill={`url(#${gradientId})`}
          />
        </AreaChart>
      </ResponsiveContainer>
    </div>
  );
}
