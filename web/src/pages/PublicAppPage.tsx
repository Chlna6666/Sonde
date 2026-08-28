import { useEffect, useState } from "react";
import { useParams } from "react-router-dom";
import {
  AppWindow,
  Github,
  Globe,
  Radio,
  Activity,
  Layers,
  Users,
  Gauge,
  ArrowUpRight,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { motion } from "motion/react";
import { api } from "../lib/api";
import { SplineAreaChart } from "../components/SplineAreaChart";
import { DonutChart } from "../components/DonutChart";
import { MultiLineChart, VersionSeriesData } from "../components/MultiLineChart";
import { BuildBarChart } from "../components/BuildBarChart";
import { PlatformIcon, getPlatformFamily, getPlatformColor } from "../components/PlatformIcon";

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

type PublicAppData = {
  id: string;
  name: string;
  slug: string;
  isPublic: boolean;
  description?: string | null;
  githubUrl?: string | null;
  websiteUrl?: string | null;
  customHeader?: string | null;
  createdAt: number;
  stats: {
    overview: {
      totalEvents: number;
      activeUsers: number;
      totalErrors: number;
      avgDailyEvents: number;
      totalUsers?: number;
      dau?: number;
      wau?: number;
      mau?: number;
    };
    growth: GrowthMetrics;
    trend: Array<{ day: string; events: number; users: number }>;
    userGrowth: UserGrowthPoint[];
    versionTimeline: VersionTimelinePoint[];
    versionSeries?: VersionSeriesData[];
    buildDistribution?: Array<{ name: string; count: number; percentage: number }>;
    appVersions: Array<{ name: string; count: number; percentage: number }>;
    launcherVersions: Array<{ name: string; count: number; percentage: number }>;
    osFamilies?: Array<{ name: string; count: number; percentage: number }>;
    operatingSystems: Array<{ name: string; count: number; percentage: number }>;
  };
};

export function PublicAppPage() {
  const { slug } = useParams<{ slug: string }>();
  const { t } = useTranslation();
  const [data, setData] = useState<PublicAppData | null>(null);
  const [selectedDays, setSelectedDays] = useState<number | "all">(30);
  const [selectedOsFamily, setSelectedOsFamily] = useState<string>("all");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");

  useEffect(() => {
    if (!slug) return;
    const query = selectedDays === "all" ? "" : `?days=${selectedDays}`;
    api<PublicAppData>(`/api/v1/public/applications/${slug}${query}`)
      .then((res) => {
        setData(res);
        setLoading(false);
      })
      .catch((cause) => {
        setError(cause instanceof Error ? cause.message : String(cause));
        setLoading(false);
      });
  }, [slug, selectedDays]);

  const filteredOperatingSystems = (data?.stats.operatingSystems ?? []).filter((os) => {
    if (selectedOsFamily === "all") return true;
    const fam = getPlatformFamily(os.name);
    return fam === selectedOsFamily.toLowerCase();
  });

  if (loading) {
    return (
      <main className="min-h-screen flex flex-col items-center justify-center bg-[var(--bg)] text-[var(--text)]">
        <div className="sonde-mark mb-3">
          <span />
        </div>
        <p className="text-xs text-[var(--muted)] font-mono">{t("common.loading")}</p>
      </main>
    );
  }

  if (error || !data) {
    return (
      <div className="min-h-screen grid place-items-center bg-[var(--bg)] p-6">
        <div className="max-w-md w-full text-center p-8 rounded-2xl bg-[var(--panel)] border border-[var(--border)] shadow-xl">
          <AppWindow size={40} className="mx-auto mb-4 text-[var(--muted)]" />
          <h2 className="text-lg font-bold text-[var(--text)] mb-2">{t("public.notFound")}</h2>
          <p className="text-xs text-[var(--muted)] font-mono">
            Application slug: <code>{slug}</code>
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="min-h-screen bg-[var(--bg)] text-[var(--text)] flex flex-col selection:bg-[var(--signal-subtle)] selection:text-[var(--signal)]">
      {/* Top Brand Header */}
      <header className="sticky top-0 z-40 border-b border-[var(--border-soft)] bg-[var(--panel-strong)]/85 backdrop-blur-2xl px-6 py-3.5 flex items-center justify-between">
        <div className="flex items-center gap-2.5">
          <div className="h-7 w-7 rounded-lg bg-[var(--signal)] flex items-center justify-center font-black text-white text-xs shadow-xs">
            S
          </div>
          <span className="font-bold text-xs tracking-wider text-[var(--text)] uppercase">
            Sonde Telemetry
          </span>
        </div>

        <div className="flex items-center gap-3">
          <span className="inline-flex items-center gap-1.5 px-3 py-1 rounded-full border border-[var(--signal)]/30 bg-[var(--signal-subtle)] text-[var(--signal)] text-xs font-bold shadow-xs">
            <Radio size={12} className="animate-pulse" />
            <span>{t("public.operational")}</span>
          </span>
        </div>
      </header>

      {/* Main Showcase Container */}
      <main className="flex-1 max-w-6xl w-full mx-auto px-4 sm:px-6 py-8">
        {/* Hero App Card */}
        <div className="rounded-3xl border border-[var(--border-soft)] bg-[var(--panel)] p-6 sm:p-8 shadow-sm mb-6 relative overflow-hidden">
          <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-6">
            <div className="flex items-start sm:items-center gap-4">
              <div className="h-16 w-16 rounded-2xl bg-[var(--signal-subtle)] border border-[var(--signal)]/20 text-[var(--signal)] flex items-center justify-center text-xl font-black shadow-inner flex-shrink-0">
                {data.name.slice(0, 2).toUpperCase()}
              </div>
              <div>
                <div className="flex items-center gap-2.5 flex-wrap">
                  <h1 className="text-2xl sm:text-3xl font-extrabold tracking-tight text-[var(--text)] m-0">
                    {data.name}
                  </h1>
                  <span className="inline-flex items-center gap-1 px-2.5 py-0.5 rounded-full border border-[var(--signal)]/30 bg-[var(--signal-subtle)] text-[10px] font-bold text-[var(--signal)]">
                    Public
                  </span>
                  {data.customHeader ? (
                    <span className="rounded-md border border-[var(--border)] bg-[var(--input-bg)] px-2 py-0.5 text-[10px] font-medium text-[var(--muted)]">
                      {data.customHeader}
                    </span>
                  ) : null}
                </div>
                <div className="mt-1 flex items-center gap-2">
                  <code className="text-xs font-mono text-[var(--muted)]">{data.slug}</code>
                </div>
              </div>
            </div>

            {/* Action Links */}
            <div className="flex items-center gap-2.5 flex-wrap">
              {data.githubUrl ? (
                <a
                  href={data.githubUrl}
                  target="_blank"
                  rel="noreferrer"
                  className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-xl border border-[var(--border)] bg-[var(--input-bg)] text-xs font-semibold text-[var(--text)] hover:bg-[var(--panel-hover)] active:scale-95 transition-all shadow-xs"
                >
                  <Github size={14} />
                  <span>{t("public.visitGithub")}</span>
                  <ArrowUpRight size={12} className="opacity-60" />
                </a>
              ) : null}
              {data.websiteUrl ? (
                <a
                  href={data.websiteUrl}
                  target="_blank"
                  rel="noreferrer"
                  className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-xl bg-[var(--signal)] text-xs font-bold text-white hover:brightness-110 active:scale-95 transition-all shadow-xs"
                >
                  <Globe size={14} />
                  <span>{t("public.visitWebsite")}</span>
                  <ArrowUpRight size={12} className="opacity-75" />
                </a>
              ) : null}
            </div>
          </div>

          {data.description ? (
            <p className="mt-5 mb-0 text-xs sm:text-sm text-[var(--muted)] leading-relaxed bg-[var(--input-bg)] p-4 rounded-2xl border border-[var(--border-soft)]">
              {data.description}
            </p>
          ) : null}
        </div>

        {/* Time Window Toolbar */}
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 mb-6">
          <div className="flex items-center gap-2">
            <span className="text-xs font-bold tracking-wide uppercase text-[var(--muted)]">
              {t("public.summary")}
            </span>
          </div>

          <div className="segmented-control self-start sm:self-auto">
            <button
              type="button"
              className={`segmented-control-item ${selectedDays === 1 ? "active" : ""}`}
              onClick={() => setSelectedDays(1)}
            >
              {selectedDays === 1 ? (
                <motion.div
                  layoutId="public-time-pill"
                  transition={{ type: "spring", stiffness: 450, damping: 32 }}
                  className="segmented-control-pill"
                />
              ) : null}
              <span className="relative z-10">{t("apps.stats24h")}</span>
            </button>
            <button
              type="button"
              className={`segmented-control-item ${selectedDays === 7 ? "active" : ""}`}
              onClick={() => setSelectedDays(7)}
            >
              {selectedDays === 7 ? (
                <motion.div
                  layoutId="public-time-pill"
                  transition={{ type: "spring", stiffness: 450, damping: 32 }}
                  className="segmented-control-pill"
                />
              ) : null}
              <span className="relative z-10">{t("apps.stats7d")}</span>
            </button>
            <button
              type="button"
              className={`segmented-control-item ${selectedDays === 30 ? "active" : ""}`}
              onClick={() => setSelectedDays(30)}
            >
              {selectedDays === 30 ? (
                <motion.div
                  layoutId="public-time-pill"
                  transition={{ type: "spring", stiffness: 450, damping: 32 }}
                  className="segmented-control-pill"
                />
              ) : null}
              <span className="relative z-10">{t("apps.stats30d")}</span>
            </button>
            <button
              type="button"
              className={`segmented-control-item ${selectedDays === "all" ? "active" : ""}`}
              onClick={() => setSelectedDays("all")}
            >
              {selectedDays === "all" ? (
                <motion.div
                  layoutId="public-time-pill"
                  transition={{ type: "spring", stiffness: 450, damping: 32 }}
                  className="segmented-control-pill"
                />
              ) : null}
              <span className="relative z-10">{t("apps.statsAll")}</span>
            </button>
          </div>
        </div>

        {/* 4 Metric Cards Ribbon */}
        <div className="grid grid-cols-2 lg:grid-cols-4 gap-4 mb-6">
          <div className="rounded-2xl border border-[var(--border-soft)] bg-[var(--panel)] p-5 shadow-sm">
            <div className="flex items-center justify-between text-[var(--muted)] mb-2">
              <span className="text-xs font-medium">{t("stats.totalUsers")}</span>
              <Users size={16} />
            </div>
            <div className="text-2xl font-extrabold font-mono text-[var(--text)]">
              {(data.stats.overview.totalUsers ?? data.stats.overview.activeUsers).toLocaleString()}
            </div>
          </div>

          <div className="rounded-2xl border border-[var(--border-soft)] bg-[var(--panel)] p-5 shadow-sm">
            <div className="flex items-center justify-between text-[var(--muted)] mb-2">
              <span className="text-xs font-medium">{t("stats.dau")}</span>
              <Activity size={16} className="text-[var(--signal)]" />
            </div>
            <div className="text-2xl font-extrabold font-mono text-[var(--signal)]">
              {(data.stats.overview.dau ?? data.stats.overview.activeUsers).toLocaleString()}
            </div>
          </div>

          <div className="rounded-2xl border border-[var(--border-soft)] bg-[var(--panel)] p-5 shadow-sm">
            <div className="flex items-center justify-between text-[var(--muted)] mb-2">
              <span className="text-xs font-medium">{t("stats.mau")}</span>
              <Gauge size={16} className="text-[var(--amber)]" />
            </div>
            <div className="text-2xl font-extrabold font-mono text-[var(--amber)]">
              {(data.stats.overview.mau ?? data.stats.overview.activeUsers).toLocaleString()}
            </div>
          </div>

          <div className="rounded-2xl border border-[var(--border-soft)] bg-[var(--panel)] p-5 shadow-sm">
            <div className="flex items-center justify-between text-[var(--muted)] mb-2">
              <span className="text-xs font-medium">{t("overview.events")}</span>
              <Layers size={16} />
            </div>
            <div className="text-2xl font-extrabold font-mono text-[var(--text)]">
              {data.stats.overview.totalEvents.toLocaleString()}
            </div>
          </div>
        </div>

        {/* Spacious 2x2 Visual Charts Grid */}
        {/* Row 1: Telemetry Stream & Version Distribution */}
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-5 mb-6">
          {/* Telemetry Launches Wave */}
          <div className="rounded-2xl border border-[var(--border-soft)] bg-[var(--panel)] p-5 shadow-sm flex flex-col justify-between min-h-[320px]">
            <div className="flex items-center justify-between mb-3">
              <div className="flex items-center gap-2">
                <h3 className="text-sm font-bold text-[var(--text)] m-0">{t("stats.overviewDash")}</h3>
                <span className="rounded-md border border-[var(--signal)]/30 bg-[var(--signal-subtle)] px-2 py-0.5 text-[10px] font-bold text-[var(--signal)]">
                  {t("public.launches")}
                </span>
              </div>
              <span className="inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-full border border-[var(--signal)]/30 bg-[var(--signal-subtle)] text-[var(--signal)] text-[10px] font-bold">
                <span className="h-1.5 w-1.5 rounded-full bg-[var(--signal)] animate-pulse" />
                {t("stats.live")}
              </span>
            </div>
            <div className="flex-1 w-full min-h-[230px]">
              <SplineAreaChart
                data={data.stats.trend.map((pt) => ({
                  label: pt.day,
                  value: pt.events,
                  secondaryValue: pt.users,
                }))}
                height="100%"
                strokeColor="#f97316"
                fillColor="#f97316"
                valueLabel={t("public.launches")}
                secondaryLabel={t("public.devices")}
              />
            </div>
          </div>

          {/* Version Distribution Donut */}
          <div className="h-full min-h-[320px]">
            <DonutChart
              items={data.stats.appVersions}
              title={t("stats.versionDonut")}
              badge={t("stats.appVersion")}
              height="100%"
            />
          </div>
        </div>

        {/* Row 2: Cross-Platform Builds & Multi-Version Growth */}
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-5 mb-6">
          {/* Cross-Platform System Builds */}
          <div className="rounded-2xl border border-[var(--border-soft)] bg-[var(--panel)] p-5 shadow-sm flex flex-col justify-between min-h-[320px]">
            <div className="flex items-center justify-between mb-3">
              <div className="flex items-center gap-2">
                <h3 className="text-sm font-bold text-[var(--text)] m-0">{t("stats.systemBuilds")}</h3>
                <span className="rounded-md border border-[var(--amber)]/30 bg-[var(--amber-subtle)] px-2 py-0.5 text-[10px] font-bold text-[var(--amber)]">
                  跨平台 Build
                </span>
              </div>
            </div>
            <div className="flex-1 w-full min-h-[230px]">
              <BuildBarChart
                items={data.stats.buildDistribution ?? []}
                height="100%"
              />
            </div>
          </div>

          {/* Multi-Version Growth Curves */}
          <div className="rounded-2xl border border-[var(--border-soft)] bg-[var(--panel)] p-5 shadow-sm flex flex-col justify-between min-h-[320px]">
            <div className="flex items-center justify-between mb-3">
              <div className="flex items-center gap-2">
                <h3 className="text-sm font-bold text-[var(--text)] m-0">{t("stats.versionCurves")}</h3>
                <span className="rounded-md border border-[#38bdf8]/30 bg-[#38bdf8]/10 px-2 py-0.5 text-[10px] font-bold text-[#38bdf8]">
                  {t("stats.compare")}
                </span>
              </div>
            </div>
            <div className="flex-1 w-full min-h-[230px]">
              <MultiLineChart
                series={data.stats.versionSeries ?? []}
                height="100%"
              />
            </div>
          </div>
        </div>

        {/* Operating Systems Matrix Card */}
        <div className="rounded-2xl border border-[var(--border-soft)] bg-[var(--panel)] p-5 sm:p-6 shadow-sm mb-8">
          <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 mb-4">
            <div className="flex items-center gap-2">
              <h3 className="text-sm font-bold text-[var(--text)] m-0">{t("apps.osFamilies")}</h3>
              <span className="text-xs text-[var(--muted)]">
                ({filteredOperatingSystems.length} {t("apps.osDetailed")})
              </span>
            </div>

            {/* Platform Family Filter Pills */}
            <div className="flex items-center gap-1.5 overflow-x-auto pb-1 scrollbar-none flex-nowrap">
              <button
                type="button"
                className={`px-2.5 py-1 rounded-xl text-xs font-semibold transition-all cursor-pointer whitespace-nowrap flex-shrink-0 ${
                  selectedOsFamily === "all"
                    ? "bg-[var(--signal)] text-white shadow-xs font-bold"
                    : "bg-[var(--input-bg)] text-[var(--muted)] hover:text-[var(--text)] border border-[var(--border-soft)]"
                }`}
                onClick={() => setSelectedOsFamily("all")}
              >
                <span>{t("apps.statsAll")}</span>
                <span className="opacity-75 font-mono text-[10px] ml-1">
                  ({data.stats.operatingSystems.length})
                </span>
              </button>

              {(data.stats.osFamilies ?? []).map((fam) => {
                const isSelected = selectedOsFamily === fam.name;
                const isLinux = fam.name === "Linux";
                return (
                  <button
                    key={fam.name}
                    type="button"
                    className={`px-2.5 py-1 rounded-xl text-xs font-semibold transition-all cursor-pointer inline-flex items-center gap-1.5 whitespace-nowrap flex-shrink-0 ${
                      isSelected
                        ? isLinux
                          ? "bg-[var(--signal)] text-white shadow-xs font-bold"
                          : "bg-[var(--amber)] text-white shadow-xs font-bold"
                        : "bg-[var(--input-bg)] text-[var(--muted)] hover:text-[var(--text)] border border-[var(--border-soft)]"
                    }`}
                    onClick={() => setSelectedOsFamily(isSelected ? "all" : fam.name)}
                  >
                    <PlatformIcon platform={fam.name} size={12} />
                    <span>{fam.name}</span>
                    <span className="opacity-75 font-mono text-[10px]">
                      {fam.count.toLocaleString()} ({fam.percentage}%)
                    </span>
                  </button>
                );
              })}
            </div>
          </div>

          {/* OS Breakdown Progress Rows */}
          <div className="space-y-2.5 max-h-56 overflow-y-auto pr-1">
            {filteredOperatingSystems.length === 0 ? (
              <p className="text-xs text-[var(--muted)] py-4 text-center">{t("apps.noOS")}</p>
            ) : (
              filteredOperatingSystems.map((os) => {
                const color = getPlatformColor(os.name);
                return (
                  <div
                    key={os.name}
                    className="flex items-center gap-3 text-xs p-2 rounded-xl bg-[var(--input-bg)] border border-[var(--border-soft)]/50"
                  >
                    <PlatformIcon platform={os.name} size={13} className="flex-shrink-0 text-[var(--muted)]" />
                    <span className="font-semibold text-[var(--text)] flex-1 truncate">{os.name}</span>
                    <div className="w-24 sm:w-48 h-2 rounded-full bg-[var(--panel-strong)] overflow-hidden flex-shrink-0">
                      <div
                        className="h-full rounded-full transition-all duration-500"
                        style={{ width: `${os.percentage}%`, backgroundColor: color }}
                      />
                    </div>
                    <span className="text-[var(--muted)] font-mono text-[10px] w-14 text-right flex-shrink-0">
                      {os.count.toLocaleString()}
                    </span>
                    <span className="font-mono text-[10px] font-bold w-12 text-right flex-shrink-0" style={{ color }}>
                      {os.percentage}%
                    </span>
                  </div>
                );
              })
            )}
          </div>
        </div>
      </main>

      {/* Footer */}
      <footer className="border-t border-[var(--border-soft)] py-6 text-center text-xs text-[var(--muted)] bg-[var(--panel-strong)]">
        <p className="m-0">
          {t("public.poweredBy")} · {new Date().getFullYear()}
        </p>
      </footer>
    </div>
  );
}
