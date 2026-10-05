import { useEffect, useState } from "react";
import { Activity, AppWindow, CheckCircle2, CircleAlert, Gauge, Radio, RefreshCw, ScrollText, Sparkles, Trash2, TrendingUp } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useOutletContext } from "react-router-dom";
import { motion } from "motion/react";
import { MetricCard } from "../components/MetricCard";
import { CustomSelect } from "../components/CustomSelect";
import { Modal } from "../components/Modal";
import { api } from "../lib/api";
import { SplineAreaChart } from "../components/SplineAreaChart";
import { DonutChart } from "../components/DonutChart";
import { MultiLineChart, VersionSeriesData } from "../components/MultiLineChart";
import { BuildBarChart } from "../components/BuildBarChart";
import { Button, Card, Badge, Skeleton } from "../components/ui";
import type { User } from "../App";

type Application = {
  id: string;
  name: string;
  ownerUserId?: string | null;
};

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
  systemLanguages?: DistributionItem[];
};

export function DashboardPage() {
  const { t } = useTranslation();
  const outletContext = useOutletContext<{ user?: User }>();
  const [user, setUser] = useState<User | null>(outletContext?.user ?? null);
  const [applications, setApplications] = useState<Application[]>([]);
  const [selectedAppId, setSelectedAppId] = useState<string>("all");
  const [appInitialized, setAppInitialized] = useState(false);
  const [data, setData] = useState<Overview | null>(null);
  const [selectedDays, setSelectedDays] = useState<number | "all">(30);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [cleanModalOpen, setCleanModalOpen] = useState(false);
  const [cleaning, setCleaning] = useState(false);
  const [cleanSuccess, setCleanSuccess] = useState("");

  useEffect(() => {
    let active = true;
    const init = async () => {
      try {
        let currentUser = outletContext?.user;
        if (!currentUser) {
          currentUser = await api<User>("/api/v1/auth/me").catch(() => null as unknown as User);
          if (active && currentUser) setUser(currentUser);
        } else if (active) {
          setUser(currentUser);
        }
        const apps = await api<Application[]>("/api/v1/admin/applications");
        if (!active) return;
        setApplications(apps);

        // Determine default application:
        // Prioritize saved per-user preference, else user's owned application
        const storageKey = currentUser?.id ? `sonde_overview_app_${currentUser.id}` : null;
        const saved = storageKey ? localStorage.getItem(storageKey) : null;
        if (saved && (saved === "all" || apps.some((a) => a.id === saved))) {
          setSelectedAppId(saved);
        } else {
          const myApps = currentUser?.id ? apps.filter((a) => a.ownerUserId === currentUser.id) : [];
          if (myApps.length > 0) {
            setSelectedAppId(myApps[0].id);
          } else {
            setSelectedAppId("all");
          }
        }
        setAppInitialized(true);
      } catch {
        if (active) {
          setAppInitialized(true);
        }
      }
    };
    void init();
    return () => {
      active = false;
    };
  }, [outletContext?.user]);

  const handleAppChange = (newAppId: string) => {
    setSelectedAppId(newAppId);
    if (user?.id) {
      localStorage.setItem(`sonde_overview_app_${user.id}`, newAppId);
    }
  };

  const load = () => {
    setLoading(true);
    setError("");
    const params = new URLSearchParams();
    if (selectedDays !== "all") {
      params.set("days", String(selectedDays));
    }
    if (selectedAppId && selectedAppId !== "all") {
      params.set("applicationId", selectedAppId);
    }
    const qs = params.toString();
    api<Overview>(`/api/v1/admin/overview${qs ? `?${qs}` : ""}`)
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
    if (appInitialized) {
      load();
    }
  }, [appInitialized, selectedDays, selectedAppId]);

  const handleCleanInvalid = async () => {
    setCleaning(true);
    setError("");
    setCleanSuccess("");
    try {
      const res = await api<{ totalDeleted: number }>("/api/v1/admin/explorer/clean-invalid", {
        method: "POST",
        body: JSON.stringify({
          applicationId: selectedAppId !== "all" ? selectedAppId : undefined,
        }),
      });
      setCleanModalOpen(false);
      setCleanSuccess(t("overview.cleanInvalidSuccess", { count: res.totalDeleted }));
      load();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t("common.error"));
    } finally {
      setCleaning(false);
    }
  };

  const timeRanges: { value: number | "all"; label: string }[] = [
    { value: 1, label: t("apps.stats24h") },
    { value: 7, label: t("apps.stats7d") },
    { value: 30, label: t("apps.stats30d") },
    { value: "all", label: t("apps.statsAll") },
  ];

  return (
    <div className="space-y-6">
      {/* Header Bar: Title + application switcher + range switcher + clean button */}
      <div className="flex flex-col lg:flex-row lg:items-center justify-between gap-4">
        <div>
          <span className="eyebrow">{t("overview.eyebrow")}</span>
          <h1 className="text-2xl sm:text-3xl font-extrabold tracking-tight text-[var(--text)] m-0 mt-0.5">
            {t("overview.title")}
          </h1>
          <p className="text-xs text-[var(--muted)] m-0 mt-1 max-w-xl">
            {t("overview.subtitle")}
          </p>
        </div>

        <div className="flex flex-wrap items-center gap-2.5">
          {applications.length > 0 ? (
            <div className="min-w-[170px] sm:min-w-[210px]">
              <CustomSelect
                label={t("overview.filterApp")}
                value={selectedAppId}
                onChange={handleAppChange}
                icon={<AppWindow size={14} />}
                options={[
                  { value: "all", label: t("overview.allApps") },
                  ...applications.map((app) => ({
                    value: app.id,
                    label: `${app.name}${app.ownerUserId === user?.id ? ` (${t("overview.myApp")})` : ""}`,
                  })),
                ]}
              />
            </div>
          ) : null}

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
                      transition={{ type: "spring", stiffness: 400, damping: 35 }}
                      className="segmented-control-pill"
                    />
                  ) : null}
                  <span className="relative z-10">{range.label}</span>
                </button>
              );
            })}
          </div>

          <Button
            variant="outline"
            size="sm"
            onClick={() => setCleanModalOpen(true)}
            icon={<Trash2 size={13} className="text-[var(--danger)]" />}
            className="h-[34px] border-[var(--border)] hover:border-[var(--danger)] hover:text-[var(--danger)] text-xs font-semibold whitespace-nowrap"
            title={t("overview.cleanInvalid")}
          >
            <span className="hidden sm:inline">{t("overview.cleanInvalid")}</span>
          </Button>
        </div>
      </div>

      {cleanSuccess ? (
        <div className="p-3.5 rounded-[var(--radius-lg)] bg-[var(--signal-subtle)] border border-[var(--signal)]/30 text-[var(--signal)] text-xs font-semibold flex items-center justify-between">
          <div className="flex items-center gap-2">
            <CheckCircle2 size={16} />
            <span>{cleanSuccess}</span>
          </div>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => setCleanSuccess("")}
          >
            {t("common.close")}
          </Button>
        </div>
      ) : null}

      {error ? (
        <div className="p-4 rounded-[var(--radius-lg)] bg-[var(--danger-subtle)] border border-[var(--danger)]/30 text-[var(--danger)] text-xs font-semibold flex items-center justify-between">
          <span>{error}</span>
          <Button
            variant="danger"
            size="sm"
            onClick={load}
          >
            {t("common.retry")}
          </Button>
        </div>
      ) : null}

      {!data && loading ? (
        <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-6 gap-4">
          {[...Array(6)].map((_, i) => (
            <Skeleton key={i} className="h-28 rounded-[var(--radius-xl)]" />
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
            <Card className="p-5 flex flex-col justify-between min-h-[340px]">
              <div className="flex items-center justify-between mb-4">
                <div className="flex items-center gap-2.5">
                  <div className="p-1.5 rounded-[var(--radius-sm)] icon-squircle-green">
                    <TrendingUp size={16} />
                  </div>
                  <div>
                    <h3 className="text-sm font-bold text-[var(--text)] m-0">{t("stats.overviewDash")}</h3>
                    <p className="text-[11px] text-[var(--muted)] m-0 mt-0.5">{t("stats.trendAnalysis")}</p>
                  </div>
                </div>
                <Badge variant="success" dot pulse>
                  {t("stats.live")}
                </Badge>
              </div>

              <div className="flex-1 w-full min-h-[240px]">
                <SplineAreaChart
                  data={data.trend.map((pt) => ({
                    label: pt.day,
                    value: pt.events,
                    secondaryValue: pt.users,
                  }))}
                  height="100%"
                  strokeColor="var(--signal)"
                  fillColor="var(--signal)"
                  valueLabel={t("explorer.events")}
                  secondaryLabel={t("public.devices")}
                />
              </div>
            </Card>

            {/* Version Distribution Donut Chart */}
            <Card className="p-5 flex flex-col justify-between min-h-[340px]">
              <DonutChart
                items={(() => {
                  const series = data.versionSeries ?? [];
                  const total = series.reduce((acc, curr) => acc + curr.totalCount, 0);
                  return series.map((s) => ({
                    name: s.version,
                    count: s.totalCount,
                    percentage: total > 0 ? Math.round((s.totalCount / total) * 1000) / 10 : 0,
                  }));
                })()}
                title={t("stats.versionDonut")}
                badge={t("stats.appVersion")}
                centerLabel={t("apps.statsTotalEvents")}
                height="100%"
              />
            </Card>
          </div>

          {/* Row 2: Cross-Platform System Builds & Version Growth Curves */}
          <div className="grid grid-cols-1 lg:grid-cols-2 gap-5">
            {/* System Build Distribution Bar Chart */}
            <Card className="p-5 flex flex-col justify-between min-h-[340px]">
              <div className="flex items-center justify-between mb-4">
                <div className="flex items-center gap-2.5">
                  <div className="p-1.5 rounded-[var(--radius-sm)] icon-squircle-amber">
                    <Sparkles size={16} />
                  </div>
                  <div>
                    <h3 className="text-sm font-bold text-[var(--text)] m-0">{t("stats.systemBuilds")}</h3>
                    <p className="text-[11px] text-[var(--muted)] m-0 mt-0.5">{t("dashboard.osBuildDesc")}</p>
                  </div>
                </div>
              </div>
              <div className="flex-1 w-full min-h-[240px]">
                <BuildBarChart
                  items={data.buildDistribution ?? []}
                  height="100%"
                />
              </div>
            </Card>

            {/* Version Growth Multi-Line Chart */}
            <Card className="p-5 flex flex-col justify-between min-h-[340px]">
              <div className="flex items-center justify-between mb-4">
                <div className="flex items-center gap-2.5">
                  <div className="p-1.5 rounded-[var(--radius-sm)] icon-squircle-cyan">
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
            </Card>
          </div>

          {/* Row 3: OS Families & System Languages */}
          <div className="grid grid-cols-1 lg:grid-cols-2 gap-5">
            {/* System Language Distribution */}
            <Card className="p-5 flex flex-col justify-between min-h-[340px]">
              <DonutChart
                items={data.systemLanguages ?? []}
                title={t("stats.systemLanguages")}
                badge={t("stats.systemLanguage")}
                centerLabel={t("stats.totalUsers")}
                height="100%"
              />
            </Card>

            {/* Operating System Families */}
            <Card className="p-5 flex flex-col justify-between min-h-[340px]">
              <DonutChart
                items={data.osFamilies ?? []}
                title={t("apps.osFamilies")}
                badge="OS"
                centerLabel={t("apps.statsTotalEvents")}
                height="100%"
              />
            </Card>
          </div>
        </>
      ) : null}

      {cleanModalOpen ? (
        <Modal
          isOpen={true}
          onClose={() => setCleanModalOpen(false)}
          title={t("overview.cleanInvalidTitle")}
          icon={<Trash2 size={18} className="text-[var(--danger)]" />}
          size="sm"
          actions={
            <div className="flex items-center justify-end gap-2">
              <Button
                variant="secondary"
                size="sm"
                onClick={() => setCleanModalOpen(false)}
                disabled={cleaning}
              >
                {t("common.cancel")}
              </Button>
              <Button
                variant="danger"
                size="sm"
                onClick={() => void handleCleanInvalid()}
                disabled={cleaning}
                icon={cleaning ? <RefreshCw size={14} className="animate-spin" /> : <Trash2 size={14} />}
              >
                {cleaning ? t("overview.cleaning") : t("overview.confirmClean")}
              </Button>
            </div>
          }
        >
          <p className="text-xs text-[var(--muted)] leading-relaxed m-0">
            {t("overview.cleanInvalidPrompt")}
          </p>
        </Modal>
      ) : null}
    </div>
  );
}
