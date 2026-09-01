import {
  Activity,
  Clock3,
  Gauge,
  History,
  Radio,
  TimerReset,
  UsersRound,
  Waypoints,
} from "lucide-react";
import {
  CartesianGrid,
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";

export type ActivityStats = {
  summary: {
    activeMillis: number;
    lifetimeActiveMillis: number;
    sessions: number;
    lifetimeSessions: number;
    measuredDevices: number;
    measurementCoveragePct: number;
    averageSessionMillis: number;
    averageActiveMillisPerDevice: number;
    stickinessPct: number;
  };
  trend: Array<{
    bucket: string;
    activeUsers: number;
    activeMillis: number;
    sessions: number;
    averageSessionMillis: number;
    cumulativeActiveMillis: number;
    cumulativeSessions: number;
    lifetimeCumulativeActiveMillis: number;
    lifetimeCumulativeSessions: number;
  }>;
};

type Props = {
  activity: ActivityStats;
};

export function ActivityStatsPanel({ activity }: Props) {
  const { summary, trend } = activity;
  const activeTrend = trend.map((point) => ({
    bucket: point.bucket,
    window: point.cumulativeActiveMillis,
    lifetime: point.lifetimeCumulativeActiveMillis,
  }));
  const sessionTrend = trend.map((point) => ({
    bucket: point.bucket,
    window: point.cumulativeSessions,
    lifetime: point.lifetimeCumulativeSessions,
  }));

  return (
    <section className="distribution-card" style={{ display: "flex", flexDirection: "column", gap: "14px" }}>
      <div className="flex items-start justify-between gap-3 flex-wrap">
        <div>
          <div className="flex items-center gap-2">
            <Activity size={16} className="text-[var(--signal)]" />
            <h3 style={{ margin: 0 }}>在线行为与累计统计</h3>
          </div>
          <p className="text-[11px] text-[var(--muted)]" style={{ margin: "5px 0 0" }}>
            仅使用 Sonde 服务端观测到的可信活动间隔计算在线时长与 Session；历史导入仅计活跃设备，不反推在线时间。
          </p>
        </div>
        <span className="status-chip">
          <i /> 服务端权威统计
        </span>
      </div>

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fit, minmax(150px, 1fr))",
          gap: "10px",
        }}
      >
        <MetricCard icon={<Clock3 size={14} />} label="窗口在线时长" value={formatDuration(summary.activeMillis)} />
        <MetricCard icon={<History size={14} />} label="累计在线时长" value={formatDuration(summary.lifetimeActiveMillis)} />
        <MetricCard icon={<Waypoints size={14} />} label="窗口 Session" value={formatCount(summary.sessions)} />
        <MetricCard icon={<TimerReset size={14} />} label="累计 Session" value={formatCount(summary.lifetimeSessions)} />
        <MetricCard icon={<Gauge size={14} />} label="平均 Session" value={formatDuration(summary.averageSessionMillis)} />
        <MetricCard
          icon={<UsersRound size={14} />}
          label="设备平均在线"
          value={formatDuration(summary.averageActiveMillisPerDevice)}
        />
        <MetricCard icon={<Radio size={14} />} label="DAU / MAU" value={`${formatPercent(summary.stickinessPct)}%`} />
        <MetricCard
          icon={<Activity size={14} />}
          label="在线测量覆盖率"
          value={`${formatPercent(summary.measurementCoveragePct)}%`}
          detail={`${formatCount(summary.measuredDevices)} 台有可信时长样本`}
        />
      </div>

      {summary.measurementCoveragePct < 100 ? (
        <div
          className="text-[11px] text-[var(--muted)]"
          style={{
            padding: "9px 11px",
            borderRadius: "10px",
            border: "1px solid var(--border-soft)",
            background: "var(--input-bg)",
          }}
        >
          在线时长均值只以产生过可信连续活动间隔的设备为样本；当前覆盖率为 {formatPercent(summary.measurementCoveragePct)}%。
        </div>
      ) : null}

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fit, minmax(320px, 1fr))",
          gap: "12px",
        }}
      >
        <CumulativeChart
          title="累计在线时长"
          data={activeTrend}
          formatter={formatDuration}
          windowLabel="窗口累计"
          lifetimeLabel="生命周期累计"
        />
        <CumulativeChart
          title="累计 Session"
          data={sessionTrend}
          formatter={formatCount}
          windowLabel="窗口累计"
          lifetimeLabel="生命周期累计"
        />
      </div>
    </section>
  );
}

function MetricCard({
  icon,
  label,
  value,
  detail,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  detail?: string;
}) {
  return (
    <div
      style={{
        padding: "11px 12px",
        borderRadius: "12px",
        border: "1px solid var(--border-soft)",
        background: "var(--input-bg)",
        minWidth: 0,
      }}
    >
      <div className="flex items-center gap-1.5 text-[var(--muted)]" style={{ fontSize: "0.68rem" }}>
        <span className="text-[var(--signal)]">{icon}</span>
        <span>{label}</span>
      </div>
      <div
        style={{
          marginTop: "5px",
          color: "var(--text)",
          fontFamily: "var(--font-mono)",
          fontWeight: 800,
          fontSize: "1rem",
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
        }}
        title={value}
      >
        {value}
      </div>
      {detail ? (
        <div className="text-[10px] text-[var(--muted)]" style={{ marginTop: "3px" }}>
          {detail}
        </div>
      ) : null}
    </div>
  );
}

function CumulativeChart({
  title,
  data,
  formatter,
  windowLabel,
  lifetimeLabel,
}: {
  title: string;
  data: Array<{ bucket: string; window: number; lifetime: number }>;
  formatter: (value: number) => string;
  windowLabel: string;
  lifetimeLabel: string;
}) {
  return (
    <div
      style={{
        border: "1px solid var(--border-soft)",
        borderRadius: "12px",
        background: "var(--panel-strong)",
        padding: "11px 10px 8px",
        minWidth: 0,
      }}
    >
      <div className="flex items-center justify-between gap-2" style={{ marginBottom: "6px" }}>
        <strong style={{ fontSize: "0.76rem" }}>{title}</strong>
        <div className="flex items-center gap-3 text-[10px] text-[var(--muted)]">
          <span className="inline-flex items-center gap-1"><i style={{ width: 7, height: 7, borderRadius: 999, background: "var(--signal)" }} />{windowLabel}</span>
          <span className="inline-flex items-center gap-1"><i style={{ width: 7, height: 7, borderRadius: 999, background: "var(--amber)" }} />{lifetimeLabel}</span>
        </div>
      </div>
      {data.length === 0 ? (
        <div className="h-[180px] flex items-center justify-center text-xs text-[var(--muted)]">暂无可信在线数据</div>
      ) : (
        <div style={{ width: "100%", height: 190 }}>
          <ResponsiveContainer width="100%" height="100%">
            <LineChart data={data} margin={{ top: 8, right: 10, left: -16, bottom: 0 }}>
              <CartesianGrid stroke="var(--border-soft)" strokeDasharray="3 3" vertical={false} />
              <XAxis
                dataKey="bucket"
                tickFormatter={compactBucket}
                tickLine={false}
                axisLine={false}
                stroke="var(--muted)"
                fontSize={9}
                minTickGap={24}
              />
              <YAxis
                tickFormatter={(value) => compactNumber(Number(value))}
                tickLine={false}
                axisLine={false}
                stroke="var(--muted)"
                fontSize={9}
                width={42}
              />
              <Tooltip
                content={({ active, payload, label }) => {
                  if (!active || !payload?.length) return null;
                  const row = payload[0]?.payload as { window: number; lifetime: number };
                  return (
                    <div className="rounded-xl border border-[var(--border)] bg-[var(--panel-strong)] p-2.5 shadow-xl text-xs">
                      <strong>{String(label)}</strong>
                      <div className="font-mono text-[var(--signal)]" style={{ marginTop: 4 }}>
                        {windowLabel}: {formatter(row.window)}
                      </div>
                      <div className="font-mono text-[var(--amber)]" style={{ marginTop: 2 }}>
                        {lifetimeLabel}: {formatter(row.lifetime)}
                      </div>
                    </div>
                  );
                }}
              />
              <Line
                type="monotone"
                dataKey="window"
                stroke="var(--signal)"
                strokeWidth={2.2}
                dot={false}
                activeDot={{ r: 3 }}
              />
              <Line
                type="monotone"
                dataKey="lifetime"
                stroke="var(--amber)"
                strokeWidth={2.2}
                strokeDasharray="5 3"
                dot={false}
                activeDot={{ r: 3 }}
              />
            </LineChart>
          </ResponsiveContainer>
        </div>
      )}
    </div>
  );
}

function formatDuration(milliseconds: number): string {
  const value = Math.max(0, Number.isFinite(milliseconds) ? milliseconds : 0);
  const totalSeconds = Math.floor(value / 1000);
  if (totalSeconds < 60) return `${totalSeconds}s`;
  const totalMinutes = Math.floor(totalSeconds / 60);
  if (totalMinutes < 60) return `${totalMinutes}m ${totalSeconds % 60}s`;
  const totalHours = Math.floor(totalMinutes / 60);
  if (totalHours < 24) return `${totalHours}h ${totalMinutes % 60}m`;
  const days = Math.floor(totalHours / 24);
  return `${days}d ${totalHours % 24}h`;
}

function formatCount(value: number): string {
  return Math.max(0, Math.floor(Number.isFinite(value) ? value : 0)).toLocaleString();
}

function formatPercent(value: number): string {
  const safe = Math.max(0, Number.isFinite(value) ? value : 0);
  return safe.toLocaleString(undefined, { maximumFractionDigits: 1 });
}

function compactBucket(value: string): string {
  if (value.length > 10) return value.slice(11);
  if (value.length > 7) return value.slice(5);
  return value;
}

function compactNumber(value: number): string {
  const absolute = Math.abs(value);
  if (absolute >= 1_000_000_000) return `${(value / 1_000_000_000).toFixed(1)}b`;
  if (absolute >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}m`;
  if (absolute >= 1_000) return `${(value / 1_000).toFixed(1)}k`;
  return String(Math.round(value));
}
