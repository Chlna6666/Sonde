import { useTranslation } from "react-i18next";
import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
} from "recharts";
import { chartColor, shortDayLabel, compactNumber, ACTIVE_DOT_HALO } from "./chartTheme";

export type VersionSeriesData = {
  version: string;
  totalCount: number;
  data: Array<{ day: string; count: number }>;
};

export function MultiLineChart({
  series,
  height = 160,
  badge = "对比",
}: {
  series: VersionSeriesData[];
  height?: number | string;
  badge?: string;
}) {
  const { t } = useTranslation();

  if (!series || series.length === 0 || !series[0]?.data || series[0].data.length === 0) {
    return (
      <div
        className="w-full flex items-center justify-center text-xs text-[var(--muted)]"
        style={{ height: typeof height === "number" ? `${height}px` : height }}
      >
        {t("apps.noVersionData")}
      </div>
    );
  }

  const days = series[0].data.map((d) => d.day);

  const chartData = days.map((day) => {
    const entry: Record<string, any> = {
      day: shortDayLabel(day),
      fullDay: day,
    };
    series.forEach((s) => {
      const pt = s.data.find((d) => d.day === day);
      entry[s.version] = pt ? pt.count : 0;
    });
    return entry;
  });

  return (
    <div className="w-full h-full min-h-[150px] flex flex-col justify-between" style={{ height: typeof height === "number" ? `${height}px` : height }}>
      <div className="flex-1 w-full relative min-h-[120px]">
        <ResponsiveContainer width="100%" height="100%">
          <LineChart data={chartData} margin={{ top: 10, right: 12, left: -16, bottom: 0 }}>
            <CartesianGrid strokeDasharray="3 3" stroke="var(--border-soft)" vertical={false} />
            <XAxis
              dataKey="day"
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
              content={({ active, payload, label }) => {
                if (active && payload && payload.length) {
                  const activeVersions = payload.filter((p) => Number(p.value) > 0);
                  return (
                    <div className="rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--bg-float)] p-2.5 shadow-xl text-xs space-y-1 max-w-[200px]">
                      <p className="font-bold text-[var(--text)] border-b border-[var(--border-soft)] pb-1">
                        {payload[0]?.payload?.fullDay || label}
                      </p>
                      {activeVersions.length > 0 ? (
                        activeVersions.map((p) => (
                          <div key={p.name} className="flex items-center justify-between gap-3 font-mono text-[11px]">
                            <span style={{ color: p.color }} className="truncate">
                              {p.name}:
                            </span>
                            <strong className="text-[var(--text)]">{Number(p.value).toLocaleString()}</strong>
                          </div>
                        ))
                      ) : (
                        <p className="text-[var(--muted)] text-[11px]">0</p>
                      )}
                    </div>
                  );
                }
                return null;
              }}
            />
            {series.map((s, idx) => (
              <Line
                key={s.version}
                type="monotone"
                dataKey={s.version}
                stroke={chartColor(idx)}
                strokeWidth={2}
                dot={false}
                activeDot={{ r: 4, strokeWidth: 2, stroke: ACTIVE_DOT_HALO, fill: chartColor(idx) }}
              />
            ))}
          </LineChart>
        </ResponsiveContainer>
      </div>

      {/* Horizontal Scrolling Version Legend */}
      <div className="flex items-center gap-1.5 overflow-x-auto py-1 border-t border-[var(--border-soft)] mt-1 scrollbar-none">
        {series.map((s, idx) => (
          <span
            key={s.version}
            className="inline-flex items-center gap-1.5 px-2 py-0.5 rounded-md bg-[var(--input-bg)] border border-[var(--border-soft)] text-[10px] text-[var(--muted)] flex-shrink-0"
          >
            <span
              className="h-2 w-2 rounded-full flex-shrink-0"
              style={{ backgroundColor: chartColor(idx) }}
            />
            <strong className="text-[var(--text)] max-w-[100px] truncate" title={s.version}>
              {s.version}
            </strong>
          </span>
        ))}
      </div>
    </div>
  );
}
