import { FormEvent, type ReactNode, useEffect, useMemo, useState } from "react";
import {
  Activity,
  ChevronLeft,
  ChevronRight,
  CircleDot,
  Copy,
  MonitorSmartphone,
  Search,
  ShieldAlert,
  TriangleAlert,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { CustomSelect } from "../components/CustomSelect";
import { Button, Card, Badge, Table, TableHeader, TableBody, TableRow, TableHead, TableCell, Input } from "../components/ui";
import { api } from "../lib/api";

type Application = { id: string; name: string };
type Environment = { id: string; name: string };
type DeviceStatus = "active" | "recent" | "offline";
type RiskLevel = "low" | "medium" | "high" | "critical";

type Device = {
  id: string;
  environmentId: string;
  status: DeviceStatus;
  riskScore: number;
  riskLevel: RiskLevel;
  lastSeenAt: number;
  lastEventAt?: number | null;
  lastMetricAt?: number | null;
  lastLogAt?: number | null;
  lastErrorAt?: number | null;
  sessionId?: string | null;
  appVersion?: string | null;
  launcherVersion?: string | null;
  os?: string | null;
  systemLanguage?: string | null;
  architecture?: string | null;
  eventItems: number;
  metricItems: number;
  logItems: number;
  errorItems: number;
  sessionChanges: number;
  appVersionChanges: number;
  launcherVersionChanges: number;
  osChanges: number;
  anomalyReasons: string[];
  lastAnomalyAt?: number | null;
};

type DevicePage = {
  items: Device[];
  page: number;
  pageSize: number;
  total: number;
  hasMore: boolean;
  summary: {
    total: number;
    active: number;
    recent: number;
    offline: number;
    highRisk: number;
    critical: number;
  };
};

export function DevicesPage() {
  const { t, i18n } = useTranslation();
  const zh = i18n.language.toLowerCase().startsWith("zh");
  const copy = useMemo(() => deviceCopy(zh), [zh]);
  const [applications, setApplications] = useState<Application[]>([]);
  const [environments, setEnvironments] = useState<Environment[]>([]);
  const [applicationId, setApplicationId] = useState("");
  const [environmentId, setEnvironmentId] = useState("");
  const [status, setStatus] = useState("");
  const [risk, setRisk] = useState("");
  const [search, setSearch] = useState("");
  const [appliedSearch, setAppliedSearch] = useState("");
  const [page, setPage] = useState(1);
  const [data, setData] = useState<DevicePage | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [copied, setCopied] = useState("");

  useEffect(() => {
    void api<Application[]>("/api/v1/admin/applications")
      .then((items) => {
        setApplications(items);
        if (items[0]) setApplicationId((current) => current || items[0].id);
      })
      .catch(showError);
  }, []);

  useEffect(() => {
    if (!applicationId) {
      setEnvironments([]);
      setEnvironmentId("");
      return;
    }
    void api<Environment[]>(`/api/v1/admin/applications/${applicationId}/environments`)
      .then((items) => {
        setEnvironments(items);
        setEnvironmentId("");
      })
      .catch(showError);
  }, [applicationId]);

  useEffect(() => {
    if (applicationId) void load();
  }, [applicationId, environmentId, status, risk, appliedSearch, page]);

  function showError(cause: unknown) {
    setError(cause instanceof Error ? cause.message : t("common.error"));
  }

  async function load() {
    if (!applicationId) return;
    setLoading(true);
    setError("");
    const query = new URLSearchParams({ page: String(page), pageSize: "50" });
    if (environmentId) query.set("environmentId", environmentId);
    if (status) query.set("status", status);
    if (risk) query.set("risk", risk);
    if (appliedSearch) query.set("search", appliedSearch);
    try {
      setData(await api<DevicePage>(`/api/v1/admin/applications/${applicationId}/devices?${query}`));
    } catch (cause) {
      showError(cause);
    } finally {
      setLoading(false);
    }
  }

  function applySearch(event: FormEvent) {
    event.preventDefault();
    setPage(1);
    setAppliedSearch(search.trim());
  }

  async function copyDeviceId(value: string) {
    await navigator.clipboard.writeText(value);
    setCopied(value);
    window.setTimeout(() => setCopied(""), 1500);
  }

  const environmentNames = useMemo(
    () => new Map(environments.map((item) => [item.id, item.name])),
    [environments],
  );

  return (
    <div className="space-y-6">
      <div className="flex flex-col gap-2 sm:flex-row sm:items-end sm:justify-between">
        <div>
          <span className="eyebrow">{copy.eyebrow}</span>
          <h1 className="m-0 mt-0.5 text-2xl font-extrabold tracking-tight text-[var(--text)] sm:text-3xl">
            {t("settings.security")}
          </h1>
          <p className="m-0 mt-1 max-w-2xl text-xs text-[var(--muted)]">{copy.subtitle}</p>
        </div>
        <div className="flex items-center gap-2 rounded-[var(--radius-sm)] border border-[var(--signal)]/30 bg-[var(--signal-subtle)] px-3 py-1.5 text-[11px] font-bold uppercase tracking-wider text-[var(--signal)]">
          <ShieldAlert size={14} />
          {copy.observeOnly}
        </div>
      </div>

      <form onSubmit={applySearch} className="glass-panel flex flex-wrap items-end gap-3.5 p-4">
        <FilterLabel label={t("migration.application")} className="min-w-[180px] flex-1">
          <CustomSelect
            label={t("migration.application")}
            value={applicationId}
            options={applications.map((item) => ({ value: item.id, label: item.name }))}
            onChange={(value) => {
              setApplicationId(value);
              setPage(1);
            }}
          />
        </FilterLabel>

        <FilterLabel label={t("migration.environment")} className="min-w-[160px] flex-1">
          <CustomSelect
            label={t("migration.environment")}
            value={environmentId}
            options={[
              { value: "", label: t("apps.allEnvironments") },
              ...environments.map((item) => ({ value: item.id, label: item.name })),
            ]}
            onChange={(value) => {
              setEnvironmentId(value);
              setPage(1);
            }}
          />
        </FilterLabel>

        <FilterLabel label={copy.status} className="min-w-[140px]">
          <CustomSelect
            label={copy.status}
            value={status}
            options={[
              { value: "", label: copy.allStatuses },
              { value: "active", label: copy.active },
              { value: "recent", label: copy.recent },
              { value: "offline", label: copy.offline },
            ]}
            onChange={(value) => {
              setStatus(value);
              setPage(1);
            }}
          />
        </FilterLabel>

        <FilterLabel label={copy.risk} className="min-w-[140px]">
          <CustomSelect
            label={copy.risk}
            value={risk}
            options={[
              { value: "", label: copy.allRisk },
              { value: "risky", label: copy.risky },
              { value: "low", label: copy.low },
              { value: "medium", label: copy.medium },
              { value: "high", label: copy.high },
              { value: "critical", label: copy.critical },
            ]}
            onChange={(value) => {
              setRisk(value);
              setPage(1);
            }}
          />
        </FilterLabel>

        <div className="min-w-[220px] flex-[2]">
          <span className="mb-1.5 block text-[11px] font-bold uppercase tracking-wider text-[var(--muted)]">
            {copy.search}
          </span>
          <div className="relative">
            <Search size={14} className="absolute left-3.5 top-1/2 -translate-y-1/2 text-[var(--faint)]" />
            <input
              value={search}
              onChange={(event) => setSearch(event.target.value)}
              placeholder={copy.searchHint}
              className="h-9 w-full rounded-xl border border-[var(--border)] bg-[var(--input-bg)] pl-9 pr-3 text-xs text-[var(--text)] outline-none focus:border-[var(--signal)]"
            />
          </div>
        </div>

        <Button type="submit" size="default" icon={<Search size={14} />}>
          {copy.apply}
        </Button>
      </form>

      {error ? (
        <div className="rounded-[var(--radius-lg)] border border-[var(--danger)]/30 bg-[var(--danger-subtle)] p-4 text-xs font-semibold text-[var(--danger)] mb-4">
          {error}
        </div>
      ) : null}

      <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-6 mb-5">
        <SummaryCard icon={MonitorSmartphone} label={copy.total} value={data?.summary.total ?? 0} />
        <SummaryCard icon={Activity} label={copy.active} value={data?.summary.active ?? 0} tone="signal" />
        <SummaryCard icon={CircleDot} label={copy.recent} value={data?.summary.recent ?? 0} />
        <SummaryCard icon={MonitorSmartphone} label={copy.offline} value={data?.summary.offline ?? 0} muted />
        <SummaryCard icon={TriangleAlert} label={copy.highRisk} value={data?.summary.highRisk ?? 0} tone="danger" />
        <SummaryCard icon={ShieldAlert} label={copy.critical} value={data?.summary.critical ?? 0} tone="danger" />
      </div>

      <section aria-busy={loading}>
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>{copy.device}</TableHead>
              <TableHead>{copy.status}</TableHead>
              <TableHead>{copy.risk}</TableHead>
              <TableHead>{copy.currentState}</TableHead>
              <TableHead>{copy.volume}</TableHead>
              <TableHead>{copy.anomalies}</TableHead>
              <TableHead>{copy.lastSeen}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {data?.items.map((device) => (
              <TableRow key={device.id} className="align-top">
                <TableCell>
                  <div className="flex items-center gap-2">
                    <code className="max-w-[150px] truncate text-[11px] font-bold text-[var(--text)]" title={device.id}>
                      {device.id.slice(0, 16)}…
                    </code>
                    <button
                      type="button"
                      className="text-[var(--muted)] hover:text-[var(--signal)] cursor-pointer"
                      onClick={() => void copyDeviceId(device.id)}
                      title={copy.copyId}
                    >
                      <Copy size={13} />
                    </button>
                  </div>
                  <div className="mt-1 text-[10px] text-[var(--faint)]">
                    {environmentNames.get(device.environmentId) ?? device.environmentId}
                    {copied === device.id ? ` · ${copy.copied}` : ""}
                  </div>
                </TableCell>
                <TableCell><StatusBadge status={device.status} copy={copy} /></TableCell>
                <TableCell><RiskBadge level={device.riskLevel} score={device.riskScore} copy={copy} /></TableCell>
                <TableCell className="text-[11px] text-[var(--muted)]">
                  <div><strong className="text-[var(--text)]">{device.appVersion ?? "-"}</strong> / {device.launcherVersion ?? "-"}</div>
                  <div className="mt-1">{device.os ?? "-"}</div>
                  {(device.systemLanguage || device.architecture) ? (
                    <div className="mt-1 flex items-center gap-1.5 flex-wrap">
                      {device.systemLanguage ? (
                        <span className="inline-flex items-center px-1.5 py-0.5 rounded bg-[var(--input-bg)] border border-[var(--border-soft)] text-[10px] font-mono text-[var(--text)]" title={copy.language}>
                          {device.systemLanguage}
                        </span>
                      ) : null}
                      {device.architecture ? (
                        <span className="inline-flex items-center px-1.5 py-0.5 rounded bg-[var(--input-bg)] border border-[var(--border-soft)] text-[10px] font-mono text-[var(--muted)]" title={copy.architecture}>
                          {device.architecture}
                        </span>
                      ) : null}
                    </div>
                  ) : null}
                  <div className="mt-1 max-w-[180px] truncate font-mono text-[10px]" title={device.sessionId ?? ""}>
                    {copy.session}: {device.sessionId ?? "-"}
                  </div>
                </TableCell>
                <TableCell className="font-mono text-[10px] text-[var(--muted)]">
                  <div>E {device.eventItems} · M {device.metricItems}</div>
                  <div className="mt-1">L {device.logItems} · X {device.errorItems}</div>
                  <div className="mt-1 text-[var(--faint)]">
                    Δ S{device.sessionChanges} V{device.appVersionChanges} O{device.osChanges}
                  </div>
                </TableCell>
                <TableCell>
                  <div className="flex max-w-[260px] flex-wrap gap-1">
                    {device.anomalyReasons.length ? device.anomalyReasons.map((reason) => (
                      <Badge key={reason} variant="danger" size="sm">
                        {reasonLabel(reason, zh)}
                      </Badge>
                    )) : <span className="text-[11px] text-[var(--faint)]">{copy.none}</span>}
                  </div>
                </TableCell>
                <TableCell className="whitespace-nowrap text-[11px] text-[var(--muted)]">
                  {formatTime(device.lastSeenAt)}
                </TableCell>
              </TableRow>
            ))}
            {!loading && data && data.items.length === 0 ? (
              <TableRow><TableCell colSpan={7} className="px-4 py-10 text-center text-xs text-[var(--muted)]">{copy.empty}</TableCell></TableRow>
            ) : null}
          </TableBody>
        </Table>

        <div className="flex items-center justify-between border border-t-0 border-[var(--card-border)] rounded-b-[var(--radius-lg)] bg-[var(--card)] px-4 py-3 text-xs text-[var(--muted)]">
          <span>{copy.total}: {data?.total ?? 0}</span>
          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              disabled={page <= 1 || loading}
              onClick={() => setPage((value) => Math.max(1, value - 1))}
              icon={<ChevronLeft size={14} />}
            >
              {copy.previous}
            </Button>
            <span className="min-w-12 text-center font-mono">{page}</span>
            <Button
              variant="outline"
              size="sm"
              disabled={!data?.hasMore || loading}
              onClick={() => setPage((value) => value + 1)}
            >
              {copy.next} <ChevronRight size={14} />
            </Button>
          </div>
        </div>
      </section>
    </div>
  );
}

function FilterLabel({ label, className = "", children }: { label: string; className?: string; children: ReactNode }) {
  return (
    <div className={className}>
      <span className="mb-1.5 block text-[11px] font-bold uppercase tracking-wider text-[var(--muted)]">{label}</span>
      {children}
    </div>
  );
}

function SummaryCard({ icon: Icon, label, value, tone, muted }: { icon: typeof Activity; label: string; value: number; tone?: "signal" | "danger"; muted?: boolean }) {
  const toneClass = tone === "danger"
    ? "text-[var(--danger)] bg-[var(--danger-subtle)]"
    : tone === "signal"
      ? "text-[var(--signal)] bg-[var(--signal-subtle)]"
      : "text-[var(--muted)] bg-[var(--input-bg)]";
  return (
    <Card className={`p-4 ${muted ? "opacity-75" : ""}`}>
      <div className={`mb-3 inline-flex rounded-xl p-2 ${toneClass}`}><Icon size={16} /></div>
      <div className="text-2xl font-extrabold tracking-tight text-[var(--text)]">{value.toLocaleString()}</div>
      <div className="mt-0.5 text-[10px] font-bold uppercase tracking-wider text-[var(--muted)]">{label}</div>
    </Card>
  );
}

function StatusBadge({ status, copy }: { status: DeviceStatus; copy: ReturnType<typeof deviceCopy> }) {
  const label = status === "active" ? copy.active : status === "recent" ? copy.recent : copy.offline;
  return (
    <Badge variant={status === "active" ? "success" : "default"} dot size="sm">
      {label}
    </Badge>
  );
}

function RiskBadge({ level, score, copy }: { level: RiskLevel; score: number; copy: ReturnType<typeof deviceCopy> }) {
  const label = level === "critical" ? copy.critical : level === "high" ? copy.high : level === "medium" ? copy.medium : copy.low;
  const variant = level === "critical" || level === "high" ? "danger" : level === "medium" ? "warning" : "success";
  return (
    <Badge variant={variant} size="sm">
      {label} · {score}
    </Badge>
  );
}

function formatTime(value: number) {
  return new Date(value).toLocaleString();
}

function reasonLabel(reason: string, zh: boolean) {
  const labels: Record<string, [string, string]> = {
    large_batch: ["Large batch", "大批量上报"],
    late_telemetry: ["Late telemetry", "延迟遥测"],
    clock_ahead: ["Clock ahead", "设备时钟超前"],
    rapid_session_change: ["Rapid session changes", "会话快速切换"],
    rapid_app_version_change: ["Rapid app version changes", "应用版本快速切换"],
    rapid_launcher_version_change: ["Rapid launcher changes", "启动器版本快速切换"],
    os_changed: ["OS changed", "操作系统变化"],
    rapid_os_change: ["Rapid OS changes", "操作系统快速切换"],
  };
  return labels[reason]?.[zh ? 1 : 0] ?? reason;
}

function deviceCopy(zh: boolean) {
  return zh ? {
    eyebrow: "安全 / 设备防刷",
    subtitle: "查看由可信签名遥测推导出的设备当前状态、活动轨迹和异常风险信号。风险分仅用于观察，不会自动封禁设备。",
    observeOnly: "观察模式 · 不自动封禁",
    status: "活动状态", allStatuses: "全部状态", active: "活跃（15分钟）", recent: "近期（24小时）", offline: "离线",
    risk: "风险等级", allRisk: "全部风险", risky: "有风险（≥20）", low: "低", medium: "中", high: "高", critical: "严重",
    search: "设备检索", searchHint: "设备哈希、Session、版本、OS 或系统语言...", apply: "应用筛选",
    total: "设备总数", highRisk: "高风险设备", device: "设备", currentState: "平台推导当前状态", volume: "可信遥测量", anomalies: "异常信号", lastSeen: "最后活动",
    session: "Session", language: "系统语言", architecture: "系统架构", copyId: "复制设备哈希", copied: "已复制", none: "无", empty: "当前筛选条件下没有设备画像。", previous: "上一页", next: "下一页",
  } : {
    eyebrow: "SECURITY / DEVICE ABUSE",
    subtitle: "Inspect server-derived current device state, activity and anomaly signals from trusted signed telemetry. Risk scores are observational and do not automatically block devices.",
    observeOnly: "Observe only · no auto-block",
    status: "Activity", allStatuses: "All statuses", active: "Active (15m)", recent: "Recent (24h)", offline: "Offline",
    risk: "Risk", allRisk: "All risk", risky: "Risky (≥20)", low: "Low", medium: "Medium", high: "High", critical: "Critical",
    search: "Device search", searchHint: "Device hash, session, version, OS or language...", apply: "Apply filters",
    total: "Total devices", highRisk: "High risk", device: "Device", currentState: "Server-derived state", volume: "Trusted telemetry", anomalies: "Anomaly signals", lastSeen: "Last seen",
    session: "Session", language: "System Language", architecture: "Architecture", copyId: "Copy device hash", copied: "Copied", none: "None", empty: "No device profiles match the current filters.", previous: "Previous", next: "Next",
  };
}
