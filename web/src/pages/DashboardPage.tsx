import { useEffect, useState } from "react";
import { Activity, AppWindow, CircleAlert, Gauge, Radio, ScrollText, Sparkles, TrendingUp } from "lucide-react";
import { useTranslation } from "react-i18next";
import { motion } from "motion/react";
import { MetricCard } from "../components/MetricCard";
import { api } from "../lib/api";
import { SplineAreaChart } from "../components/SplineAreaChart";
import { DonutChart } from "../components/DonutChart";
import { MultiLineChart, VersionSeriesData } from "../components/MultiLineChart";
import { BuildBarChart } from "../components/BuildBarChart";

type VersionShare = {
  version: string;
  count: number;
  percentage: number;
};

type VersionTimelinePoint = {
  bucket: string;
  totalEvents: number;
  versions: VersionShare[];
};

type UserGrowthPoint = {
  bucket: string;
  newUsers: number;
  cumulativeUsers: number;
  activeUsers: number;
};

type GrowthMetrics = {
  eventsGrowthPct: number | null;
  usersGrowthPct: number | null;
  newUsers: number;
  returningUsers: number;
};

type DailyTrendPoint = {
  day: string;
  events: number;
  users: number;
};

type DistributionItem = {
  name: string;
  count: number;
  percentage: number;
};

type Overview = {
  applications: number;
  events24h: number;
  metrics24h: number;
  logs24h: number;
  errors24h: number;
  activeUsers24h: number;
  totalUsers?: number;
  dau?: number;
  wau?: number;
  mau?: number;
  growth: GrowthMetrics;
  trend: DailyTrendPoint[];
  userGrowth: UserGrowthPoint[];
  versionTimeline: VersionTimelinePoint[];
  versionSeries?: VersionSeriesData[];
  osFamilies?: DistributionItem[];
  operatingSystems?: DistributionItem[];
  buildDistribution?: DistributionItem[];
};

export function DashboardPage() {
  const { t } = useTranslation();
  const [data, setData] = useState<Overview | null>(null);
  const [selectedDays, setSelectedDays] = useState<number | "all">(30);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");

  const load = () => {
    setLoading(true);
    const query = selectedDays === "all" ? "" : `?days=${selectedDays}`;
    api<Overview>(`/api/v1/admin/overview${query}`)
      .then((res) => {
        setData(res);
        setLoading(false);
      })
      .catch((cause) => {
        setError(cause instanceof Error ? cause.message : t("common.error"));
        setLoading(false);
      });
  };

  useEffect(() => {
    load();
  }, [selectedDays]);

  const timeRanges: { value: number | "all"; label: string }[] = [
    { value: 1, label: t("apps.stats24h") },
    { value: 7, label: t("apps.stats7d") },
    { value: 30, label: t("apps.stats30d") },
    { value: "all", label: t("apps.statsAll") },
  ];

  return (
    <div className="space-y-6">
      {/* Header Bar: Title + Apple Segmented Range Switcher */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
        <div>
          <span className="eyebrow">{t("overview.eyebrow")}</span>
          <h1 className="text-2xl sm:text-3xl font-extrabold tracking-tight text-[var(--text)] m-0 mt-0.5">
            {t("overview.title")}
          </h1>
          <p className="text-xs text-[var(--muted)] m-0 mt-1 max-w-xl">
            {t("overview.subtitle")}
          </p>
        </div>

        <div className="segmented-control self-start sm:self-auto flex-shrink-0">
          {timeRanges.map((range) => {
            const active = selectedDays === range.value;
            return (
              <button
                key={String(range.value)}
                type="button"
                className={`segmented-control-item ${active ? "active" : ""}`}
                onClick={() => setSelectedDays(range.value)}
              >
                {active ? (
                  <motion.div
                    layoutId="dashboard-time-pill"
                    transition={{ type: "spring", stiffness: 450, damping: 32 }}
                    className="segmented-control-pill"
                  />
                ) : null}
                <span className="relative z-10">{range.label}</span>
              </button>
            );
          })}
        </div>
      </div>

      {error ? (
        <div className="p-4 rounded-2xl bg-[var(--danger-subtle)] border border-[var(--danger)]/30 text-[var(--danger)] text-xs font-semibold flex items-center justify-between">
          <span>{error}</span>
          <button
            type="button"
            onClick={load}
            className="px-3 py-1 bg-[var(--danger)] text-white rounded-lg text-xs font-bold hover:brightness-110 cursor-pointer"
          >
            {t("common.retry")}
          </button>
        </div>
      ) : null}

      {!data && loading ? (
        <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-6 gap-4">
          {[...Array(6)].map((_, i) => (
            <div key={i} className="h-28 rounded-2xl bg-[var(--panel-strong)] animate-pulse" />
          ))}
        </div>
      ) : data ? (
        <>
          {/* Key Metrics Grid (Responsive 2 to 6 columns) */}
          <section className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-6 gap-4" aria-label="Key telemetry metrics">
            <MetricCard
              label={t("overview.apps")}
              value={data.applications}
              icon={AppWindow}
              accent="indigo"
            />
            <MetricCard
              label={t("stats.totalUsers")}
              value={data.totalUsers ?? data.activeUsers24h}
              icon={Activity}
              accent="blue"
              trend={data.growth?.usersGrowthPct !== null && data.growth?.usersGrowthPct !== undefined ? { value: data.growth.usersGrowthPct } : undefined}
            />
            <MetricCard
              label={t("stats.dau")}
              value={data.dau ?? data.activeUsers24h}
              icon={Radio}
              accent="green"
            />
            <MetricCard
              label={t("stats.mau")}
              value={data.mau ?? data.activeUsers24h}
              icon={Gauge}
              accent="purple"
            />
            <MetricCard
              label={t("overview.events")}
              value={data.events24h}
              icon={ScrollText}
              accent="cyan"
              trend={data.growth?.eventsGrowthPct !== null && data.growth?.eventsGrowthPct !== undefined ? { value: data.growth.eventsGrowthPct } : undefined}
            />
            <MetricCard
              label={t("overview.errors")}
              value={data.errors24h}
              icon={CircleAlert}
              accent="red"
            />
          </section>

          {/* Row 1: Telemetry Stream Trend & Version Adoption Donut */}
          <div className="grid grid-cols-1 lg:grid-cols-2 gap-5">
            {/* Overview Wave Chart */}
            <div className="glass-panel p-6 flex flex-col justify-between min-h-[340px]">
              <div className="flex items-center justify-between mb-4">
                <div className="flex items-center gap-2.5">
                  <div className="p-2 rounded-xl icon-squircle-green">
                    <TrendingUp size={16} />
                  </div>
                  <div>
                    <h3 className="text-sm font-bold text-[var(--text)] m-0">{t("stats.overviewDash")}</h3>
                    <p className="text-[11px] text-[var(--muted)] m-0 mt-0.5">{t("stats.trendAnalysis")}</p>
                  </div>
                </div>
                <span className="inline-flex items-center gap-1.5 px-3 py-1 rounded-full border border-[var(--signal)]/30 bg-[var(--signal-subtle)] text-[var(--signal)] text-[11px] font-bold">
                  <span className="h-2 w-2 rounded-full bg-[var(--signal)] animate-pulse" />
                  {t("stats.live")}
                </span>
              </div>

              <div className="flex-1 w-full min-h-[240px]">
                <SplineAreaChart
                  data={data.trend.map((pt) => ({
                    label: pt.day,
                    value: pt.events,
                    secondaryValue: pt.users,
                  }))}
                  height="100%"
                  strokeColor="#30d158"
                  fillColor="#30d158"
                  valueLabel={t("explorer.events")}
                  secondaryLabel={t("public.devices")}
                />
              </div>
            </div>

            {/* Version Distribution Donut Chart */}
            <div className="glass-panel p-6 flex flex-col justify-between min-h-[340px]">
              <DonutChart
                items={data.versionSeries?.map((s) => ({
                  name: s.version,
                  count: s.totalCount,
                  percentage: data.events24h > 0 ? Math.round((s.totalCount / data.events24h) * 1000) / 10 : 0,
                })) ?? []}
                title={t("stats.versionDonut")}
                badge={t("stats.appVersion")}
                height="100%"
              />
            </div>
          </div>

          {/* Row 2: Cross-Platform System Builds & Version Growth Curves */}
          <div className="grid grid-cols-1 lg:grid-cols-2 gap-5">
            {/* System Build Distribution Bar Chart */}
            <div className="glass-panel p-6 flex flex-col justify-between min-h-[340px]">
              <div className="flex items-center justify-between mb-4">
                <div className="flex items-center gap-2.5">
                  <div className="p-2 rounded-xl icon-squircle-amber">
                    <Sparkles size={16} />
                  </div>
                  <div>
                    <h3 className="text-sm font-bold text-[var(--text)] m-0">{t("stats.systemBuilds")}</h3>
                    <p className="text-[11px] text-[var(--muted)] m-0 mt-0.5">跨平台 OS 与构建版本分布</p>
                  </div>
                </div>
              </div>
              <div className="flex-1 w-full min-h-[240px]">
                <BuildBarChart
                  items={data.buildDistribution ?? []}
                  height="100%"
                />
              </div>
            </div>

            {/* Version Growth Multi-Line Chart */}
            <div className="glass-panel p-6 flex flex-col justify-between min-h-[340px]">
              <div className="flex items-center justify-between mb-4">
                <div className="flex items-center gap-2.5">
                  <div className="p-2 rounded-xl icon-squircle-cyan">
                    <Activity size={16} />
                  </div>
                  <div>
                    <h3 className="text-sm font-bold text-[var(--text)] m-0">{t("stats.versionCurves")}</h3>
                    <p className="text-[11px] text-[var(--muted)] m-0 mt-0.5">{t("stats.compare")}</p>
                  </div>
                </div>
              </div>
              <div className="flex-1 w-full min-h-[240px]">
                <MultiLineChart
                  series={data.versionSeries ?? []}
                  height="100%"
                />
              </div>
            </div>
          </div>
        </>
      ) : null}
    </div>
  );
}
