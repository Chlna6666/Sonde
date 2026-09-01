import { FormEvent, useEffect, useState } from "react";
import {
  Activity,
  AppWindow,
  Ban,
  BarChart3,
  Check,
  CheckCircle2,
  Copy,
  Download,
  ExternalLink,
  Globe,
  KeyRound,
  Plus,
  RefreshCw,
  Settings,
  Shield,
  Sliders,
  ShieldAlert,
  ShieldCheck,
  Trash2,
  Upload,
  UserPlus,
  TrendingUp,
  Users,
  X,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { Modal } from "../components/Modal";
import {
  SiRust,
  SiTypescript,
  SiGo,
  SiPython,
  SiDotnet,
  SiCplusplus,
  SiCurl,
} from "react-icons/si";
import { Infinity as InfinityIcon } from "lucide-react";
import { IntegrationDocsModal } from "../components/IntegrationDocsModal";
import { Zap } from "lucide-react";
import { SplineAreaChart } from "../components/SplineAreaChart";
import { DonutChart } from "../components/DonutChart";
import { MultiLineChart, VersionSeriesData } from "../components/MultiLineChart";
import { BuildBarChart } from "../components/BuildBarChart";
import { ActivityStatsPanel, type ActivityStats } from "../components/ActivityStatsPanel";
import { PlatformIcon, detectPlatform } from "../components/PlatformIcon";
import { api } from "../lib/api";

type Application = {
  id: string;
  name: string;
  slug: string;
  retentionDays: number;
  ownerUserId?: string | null;
  isPublic?: boolean;
  description?: string | null;
  githubUrl?: string | null;
  websiteUrl?: string | null;
  customHeader?: string | null;
  createdAt: number;
};

type Environment = {
  id: string;
  applicationId: string;
  name: string;
  slug: string;
};

type ApiKey = {
  id: string;
  applicationId: string;
  environmentId: string;
  environmentName: string;
  name: string;
  keyPrefix: string;
  scopes: string[];
  expiresAt?: number | null;
  lastUsedAt?: number | null;
  revokedAt?: number | null;
  createdAt: number;
  isActive: boolean;
};

type AppMember = {
  userId: string;
  username: string;
  email: string;
  role: string;
  grantedAt: number;
};

type UserSummary = {
  id: string;
  username: string;
  email: string;
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

type AppTelemetryStats = {
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
  activity: ActivityStats;
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

const VERSION_PALETTE = [
  "var(--signal)",
  "#60a5fa",
  "#34d399",
  "#fbbf24",
  "#a78bfa",
  "#f472b6",
  "#38bdf8",
  "#fb923c",
  "#a3e635",
];

export function ApplicationsPage() {
  const { t } = useTranslation();
  const [applications, setApplications] = useState<Application[]>([]);
  const [creating, setCreating] = useState(false);
  const [importing, setImporting] = useState(false);
  const [error, setError] = useState("");
  const [globalKeyReveal, setGlobalKeyReveal] = useState("");

  const [managingApp, setManagingApp] = useState<Application | null>(null);
  const [statsApp, setStatsApp] = useState<Application | null>(null);
  
  const load = () => {
    api<Application[]>("/api/v1/admin/applications")
      .then(setApplications)
      .catch((cause) => setError(cause instanceof Error ? cause.message : String(cause)));
  };

  useEffect(load, []);

  const create = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setError("");
    const form = new FormData(event.currentTarget);
    try {
      const name = String(form.get("name"));
      const slug = String(form.get("slug"));
      const created = await api<{ id: string; environmentId: string }>("/api/v1/admin/applications", {
        method: "POST",
        body: JSON.stringify({ name, slug }),
      });
      const key = await api<{ key: string }>(`/api/v1/admin/applications/${created.id}/keys`, {
        method: "POST",
        body: JSON.stringify({
          environmentId: created.environmentId,
          name: "Default Ingest Key",
          scopes: ["telemetry.events", "telemetry.metrics", "telemetry.logs", "telemetry.errors"],
        }),
      });
      setGlobalKeyReveal(key.key);
      setCreating(false);
      load();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t("common.error"));
    }
  };

  const handleExportApp = async (app: Application) => {
    try {
      const data = await api<unknown>(`/api/v1/admin/applications/${app.id}/export`);
      const blob = new Blob([JSON.stringify(data, null, 2)], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `${app.slug}-export.sonde.json`;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      URL.revokeObjectURL(url);
    } catch (cause) {
      alert(cause instanceof Error ? cause.message : t("common.error"));
    }
  };

  return (
    <div className="page enter-page">
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 mb-6">
        <div>
          <h2 className="text-xl font-bold tracking-tight text-[var(--text)] m-0">{t("apps.title")}</h2>
          <p className="text-xs text-[var(--muted)] mt-0.5">{t("apps.subtitle")}</p>
        </div>
        <div className="flex items-center gap-2">
          <button
            type="button"
            className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-xl border border-[var(--border)] bg-[var(--panel)] text-xs font-semibold text-[var(--text)] hover:bg-[var(--panel-hover)] active:scale-95 transition-all cursor-pointer shadow-sm"
            onClick={() => setImporting(true)}
          >
            <Upload size={14} aria-hidden="true" />
            <span>{t("apps.importApp")}</span>
          </button>
          <button
            type="button"
            className="inline-flex items-center gap-1.5 px-3.5 py-1.5 rounded-xl bg-[var(--signal)] text-xs font-bold text-white shadow-sm hover:brightness-110 active:scale-95 transition-all cursor-pointer"
            onClick={() => setCreating(true)}
          >
            <Plus size={15} aria-hidden="true" />
            <span>{t("apps.new")}</span>
          </button>
        </div>
      </div>

      {globalKeyReveal ? (
        <div className="key-reveal" role="status">
          <div>
            <KeyRound aria-hidden="true" />
            <div>
              <strong>{t("apps.keyGenerated")}</strong>
              <code>{globalKeyReveal}</code>
              <small>{t("apps.keyGeneratedHint")}</small>
            </div>
          </div>
          <button
            type="button"
            onClick={() => navigator.clipboard.writeText(globalKeyReveal)}
            aria-label="Copy API key"
          >
            <Copy size={16} />
          </button>
        </div>
      ) : null}

      {error ? (
        <div className="error-panel">
          <p>{error}</p>
          <button onClick={load}>{t("common.retry")}</button>
        </div>
      ) : null}

      {creating ? (
        <form className="inline-create" onSubmit={create}>
          <Field name="name" label={t("apps.name")} placeholder="e.g. Flight Deck" required />
          <Field
            name="slug"
            label={t("apps.slug")}
            placeholder="flight-deck"
            pattern="[a-z0-9]+(?:-[a-z0-9]+)*"
            required
          />
          <div>
            <button className="primary-button compact">{t("common.create")}</button>
            <button
              type="button"
              className="secondary-button compact"
              onClick={() => setCreating(false)}
            >
              {t("common.cancel")}
            </button>
          </div>
        </form>
      ) : null}

      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
        {applications.map((application) => {
          const isPermanent = !application.retentionDays || application.retentionDays <= 0;
          return (
            <article
              key={application.id}
              className="glass-panel glass-panel-interactive p-6 flex flex-col justify-between group overflow-hidden"
            >
              <div>
                {/* Card Header: Icon + Name + Public Badge + Slug + Actions */}
                <div className="flex items-start justify-between gap-2 mb-3">
                  <div className="flex items-center gap-3.5 min-w-0">
                    <div className="flex h-12 w-12 items-center justify-center rounded-2xl bg-[var(--signal-subtle)] text-[var(--signal)] border border-[var(--signal)]/25 shadow-xs flex-shrink-0 font-black text-lg">
                      {application.name.slice(0, 2).toUpperCase()}
                    </div>
                    <div className="min-w-0">
                      <div className="flex items-center gap-2 flex-wrap">
                        <h3 className="text-base font-bold text-[var(--text)] m-0 leading-tight truncate">
                          {application.name}
                        </h3>
                        {application.isPublic ? (
                          <span className="inline-flex items-center gap-1 rounded-full border border-[var(--signal)]/30 bg-[var(--signal-subtle)] px-2 py-0.5 text-[10px] font-bold text-[var(--signal)] flex-shrink-0">
                            <Globe size={10} />
                            Public
                          </span>
                        ) : (
                          <span className="inline-flex items-center gap-1 rounded-full border border-[var(--border)] bg-[var(--input-bg)] px-2 py-0.5 text-[10px] font-medium text-[var(--muted)] flex-shrink-0">
                            Private
                          </span>
                        )}
                      </div>
                      <div className="flex items-center gap-1.5 mt-1">
                        <code className="text-xs font-mono text-[var(--muted)]">{application.slug}</code>
                        <button
                          type="button"
                          className="text-[var(--muted)] hover:text-[var(--text)] p-0.5 cursor-pointer rounded transition-colors"
                          title={t("common.copy")}
                          onClick={() => navigator.clipboard.writeText(application.slug)}
                        >
                          <Copy size={11} />
                        </button>
                      </div>
                    </div>
                  </div>

                  {/* Header Utility Icon Actions */}
                  <div className="flex items-center gap-1 flex-shrink-0">
                    {application.isPublic ? (
                      <a
                        href={`/p/${application.slug}`}
                        target="_blank"
                        rel="noreferrer"
                        className="p-1.5 rounded-xl text-[var(--muted)] hover:text-[var(--signal)] hover:bg-[var(--signal-subtle)] active:scale-95 transition-all"
                        title={t("apps.previewPublic")}
                      >
                        <ExternalLink size={15} />
                      </a>
                    ) : null}
                    <button
                      type="button"
                      className="p-1.5 rounded-xl text-[var(--muted)] hover:text-[var(--text)] hover:bg-[var(--input-bg)] active:scale-95 transition-all cursor-pointer"
                      title={t("apps.exportApp")}
                      onClick={() => handleExportApp(application)}
                    >
                      <Download size={15} />
                    </button>
                  </div>
                </div>

                {/* Quick Telemetry Indicators Ribbon */}
                <div className="grid grid-cols-2 gap-3 my-5">
                  <div className="p-3.5 rounded-2xl bg-[var(--input-bg)] border border-[var(--border-soft)]">
                    <span className="text-[11px] font-medium text-[var(--muted)] block mb-1">
                      {t("apps.retention")}
                    </span>
                    <div className="flex items-baseline gap-1">
                      {isPermanent ? (
                        <strong className="text-sm font-bold text-[var(--signal)] inline-flex items-center gap-1 mt-0.5">
                          <InfinityIcon size={15} />
                          <span>永久保存</span>
                        </strong>
                      ) : (
                        <div className="flex items-baseline gap-1">
                          <strong className="text-lg font-extrabold font-mono text-[var(--text)]">
                            {application.retentionDays}
                          </strong>
                          <span className="text-xs text-[var(--muted)] font-medium">天</span>
                        </div>
                      )}
                    </div>
                  </div>
                  <div className="p-3.5 rounded-2xl bg-[var(--input-bg)] border border-[var(--border-soft)]">
                    <span className="text-[11px] font-medium text-[var(--muted)] block mb-1">
                      {t("apps.status")}
                    </span>
                    <div className="flex items-center gap-2 mt-1">
                      <span className="h-2 w-2 rounded-full bg-[var(--signal)] shadow-[0_0_8px_var(--signal)] animate-pulse" />
                      <strong className="text-xs font-bold text-[var(--signal)]">{t("apps.active")}</strong>
                    </div>
                  </div>
                </div>
              </div>

              {/* Pixel-Perfect 2-Column Equal Grid Actions (100% Contained, Zero Overflow) */}
              <div className="grid grid-cols-2 gap-3 pt-4 border-t border-[var(--border-soft)]">
                <button
                  type="button"
                  className="h-10 px-3 rounded-xl bg-[var(--signal)] text-white text-xs font-bold inline-flex items-center justify-center gap-1.5 whitespace-nowrap hover:brightness-110 active:scale-97 transition-all cursor-pointer shadow-xs"
                  onClick={() => setStatsApp(application)}
                >
                  <BarChart3 size={14} />
                  <span>{t("apps.stats")}</span>
                </button>
                <button
                  type="button"
                  className="h-10 px-3 rounded-xl border border-[var(--border)] bg-[var(--input-bg)] text-xs font-semibold text-[var(--text)] inline-flex items-center justify-center gap-1.5 whitespace-nowrap hover:bg-[var(--panel-hover)] active:scale-97 transition-all cursor-pointer shadow-xs"
                  onClick={() => setManagingApp(application)}
                >
                  <Settings size={14} />
                  <span>配置与 SDK</span>
                </button>
              </div>
            </article>
          );
        })}

        {applications.length === 0 && !creating ? (
          <div className="col-span-full py-16 text-center text-[var(--muted)]">
            <Activity size={32} className="mx-auto mb-2 opacity-50" />
            <h3 className="text-base font-bold text-[var(--text)]">{t("apps.empty")}</h3>
            <p className="text-xs mt-1">{t("apps.emptyDesc")}</p>
          </div>
        ) : null}
      </div>

      {managingApp ? (
        <ManageAppModal
          application={managingApp}
          onClose={() => {
            setManagingApp(null);
            load();
          }}
          onExport={() => handleExportApp(managingApp)}
        />
      ) : null}

      
      {statsApp ? (
        <AppStatsModal
          application={statsApp}
          onClose={() => setStatsApp(null)}
        />
      ) : null}

      {importing ? (
        <ImportAppModal
          onClose={() => setImporting(false)}
          onSuccess={() => {
            setImporting(false);
            load();
          }}
        />
      ) : null}
    </div>
  );
}

function Field({
  name,
  label,
  placeholder,
  pattern,
  defaultValue,
  required,
}: {
  name: string;
  label: string;
  placeholder?: string;
  pattern?: string;
  defaultValue?: string;
  required?: boolean;
}) {
  return (
    <label className="field">
      <span>{label}</span>
      <input
        name={name}
        placeholder={placeholder}
        pattern={pattern}
        defaultValue={defaultValue}
        required={required}
      />
    </label>
  );
}

function ImportAppModal({
  onClose,
  onSuccess,
}: {
  onClose: () => void;
  onSuccess: () => void;
}) {
  const { t } = useTranslation();
  const [fileContent, setFileContent] = useState<unknown | null>(null);
  const [fileName, setFileName] = useState("");
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);

  const handleFile = (e: React.ChangeEvent<HTMLInputElement>) => {
    setError("");
    const file = e.target.files?.[0];
    if (!file) return;
    setFileName(file.name);
    const reader = new FileReader();
    reader.onload = (evt) => {
      try {
        const text = evt.target?.result as string;
        const parsed = JSON.parse(text);
        if (!parsed.application || !parsed.application.name) {
          throw new Error("Invalid Sonde application export file");
        }
        setFileContent(parsed);
      } catch (err) {
        setError(err instanceof Error ? err.message : "Failed to parse JSON file");
        setFileContent(null);
      }
    };
    reader.readAsText(file);
  };

  const handleImport = async () => {
    if (!fileContent) return;
    setLoading(true);
    setError("");
    try {
      await api("/api/v1/admin/applications/import", {
        method: "POST",
        body: JSON.stringify(fileContent),
      });
      onSuccess();
    } catch (err) {
      setError(err instanceof Error ? err.message : t("common.error"));
      setLoading(false);
    }
  };

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const appData = (fileContent as any)?.application;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const telemetryData = (fileContent as any)?.telemetry;

  return (
    <Modal
      size="md"
      title={t("apps.importApp")}
      icon={<Upload size={20} />}
      onClose={onClose}
      footer={
        <div className="flex justify-end gap-3 w-full">
          <button type="button" className="secondary-button compact" onClick={onClose}>
            {t("common.cancel")}
          </button>
          <button
            type="button"
            className="primary-button compact"
            disabled={!fileContent || loading}
            onClick={handleImport}
          >
            {loading ? t("common.loading") : t("migration.execute")}
          </button>
        </div>
      }
    >
      <p className="text-xs text-muted" style={{ margin: 0 }}>
        {t("apps.importAppSubtitle")}
      </p>

      <label className="field">
        <span>{t("apps.importSelectFile")}</span>
        <input
          type="file"
          accept=".json,.sonde.json"
          onChange={handleFile}
          style={{ padding: "8px" }}
        />
      </label>

      {error ? <div className="form-error">{error}</div> : null}

      {appData ? (
        <div
          style={{
            background: "var(--input-bg)",
            border: "1px solid var(--border)",
            borderRadius: "8px",
            padding: "16px",
            display: "flex",
            flexDirection: "column",
            gap: "8px",
            fontSize: "0.82rem",
          }}
        >
          <div className="flex justify-between">
            <span className="text-muted">App Name:</span>
            <strong>{appData.name}</strong>
          </div>
          <div className="flex justify-between">
            <span className="text-muted">Slug:</span>
            <code>{appData.slug}</code>
          </div>
          <div className="flex justify-between">
            <span className="text-muted">Events to restore:</span>
            <span>{telemetryData?.events?.length ?? 0}</span>
          </div>
          <div className="flex justify-between">
            <span className="text-muted">Metric Points:</span>
            <span>{telemetryData?.metricPoints?.length ?? 0}</span>
          </div>
          <div className="flex justify-between">
            <span className="text-muted">Logs:</span>
            <span>{telemetryData?.logs?.length ?? 0}</span>
          </div>
        </div>
      ) : null}
    </Modal>
  );
}

function ManageAppModal({
  application,
  onClose,
  onExport,
}: {
  application: Application;
  onClose: () => void;
  onExport: () => void;
}) {
  const { t } = useTranslation();
  const [activeTab, setActiveTab] = useState<"integration" | "keys" | "members" | "public" | "settings">("integration");
  const [environments, setEnvironments] = useState<Environment[]>([]);
  const [apiKeys, setApiKeys] = useState<ApiKey[]>([]);
  const [members, setMembers] = useState<AppMember[]>([]);
  const [allUsers, setAllUsers] = useState<UserSummary[]>([]);
  const [activeKey, setActiveKey] = useState<ApiKey | null>(null);
  const [revealedKey, setRevealedKey] = useState<string>("");
  const [codeSnippetLang, setCodeSnippetLang] = useState<"rs" | "ts" | "go" | "py" | "csharp" | "cpp" | "curl">("rs");
  const [disableLogsOption, setDisableLogsOption] = useState(false);
  const [captureErrorsOption, setCaptureErrorsOption] = useState(true);
  const [useEphemeralTokenOption, setUseEphemeralTokenOption] = useState(true);
  const [customUserAgent, setCustomUserAgent] = useState("");
  const [snippetCopied, setSnippetCopied] = useState(false);

  // Create Key state
  const [newKeyName, setNewKeyName] = useState("");
  const [newKeyEnvId, setNewKeyEnvId] = useState("");
  const [newKeyScopes, setNewKeyScopes] = useState<string[]>([
    "telemetry.events",
    "telemetry.metrics",
    "telemetry.logs",
    "telemetry.errors",
  ]);

  const toggleScope = (scope: string, enabled: boolean) => {
    if (enabled) {
      setNewKeyScopes((prev) => Array.from(new Set([...prev, scope])));
    } else {
      setNewKeyScopes((prev) => prev.filter((s) => s !== scope));
    }
  };
  const [creatingKey, setCreatingKey] = useState(false);

  // Add Member state
  const [selectedUserId, setSelectedUserId] = useState("");
  const [selectedRole, setSelectedRole] = useState("Manager");

  // Public Showcase Settings
  const [isPublic, setIsPublic] = useState(application.isPublic ?? false);
  const [description, setDescription] = useState(application.description ?? "");
  const [githubUrl, setGithubUrl] = useState(application.githubUrl ?? "");
  const [websiteUrl, setWebsiteUrl] = useState(application.websiteUrl ?? "");
  const [customHeader, setCustomHeader] = useState(application.customHeader ?? "");

  const origin = window.location.origin;
  const ingestUrl = `${origin}/api/v1/telemetry/ingest`;
  const eventsUrl = `${origin}/api/v1/telemetry/events`;

  const loadData = () => {
    api<Environment[]>(`/api/v1/admin/applications/${application.id}/environments`)
      .then((envs) => {
        setEnvironments(envs);
        if (envs.length > 0 && !newKeyEnvId) {
          setNewKeyEnvId(envs[0].id);
        }
      })
      .catch(() => {});

    api<ApiKey[]>(`/api/v1/admin/applications/${application.id}/keys`)
      .then((keys) => {
        setApiKeys(keys);
        const active = keys.find((k) => k.isActive);
        if (active) setActiveKey(active);
      })
      .catch(() => {});

    api<AppMember[]>(`/api/v1/admin/applications/${application.id}/members`)
      .then(setMembers)
      .catch(() => {});

    api<UserSummary[]>("/api/v1/admin/users")
      .then(setAllUsers)
      .catch(() => {});
  };

  useEffect(loadData, [application.id]);

  const handleCreateKey = async (e: FormEvent) => {
    e.preventDefault();
    if (!newKeyName || !newKeyEnvId) return;
    try {
      const res = await api<{ id: string; key: string }>(`/api/v1/admin/applications/${application.id}/keys`, {
        method: "POST",
        body: JSON.stringify({
          environmentId: newKeyEnvId,
          name: newKeyName,
          scopes: ["ingest"],
        }),
      });
      setRevealedKey(res.key);
      setNewKeyName("");
      setCreatingKey(false);
      loadData();
    } catch (cause) {
      alert(cause instanceof Error ? cause.message : t("common.error"));
    }
  };

  const handleRevokeKey = async (keyId: string) => {
    if (!window.confirm(t("apps.revokeConfirm"))) return;
    try {
      await api(`/api/v1/admin/applications/${application.id}/keys/${keyId}/revoke`, {
        method: "POST",
      });
      loadData();
    } catch (cause) {
      alert(cause instanceof Error ? cause.message : t("common.error"));
    }
  };

  const handleDeleteKey = async (keyId: string) => {
    if (!window.confirm(t("apps.deleteKeyConfirm"))) return;
    try {
      await api(`/api/v1/admin/applications/${application.id}/keys/${keyId}`, {
        method: "DELETE",
      });
      loadData();
    } catch (cause) {
      alert(cause instanceof Error ? cause.message : t("common.error"));
    }
  };

  const handleClearRevokedKeys = async () => {
    if (!window.confirm(t("apps.clearRevokedKeysConfirm"))) return;
    try {
      await api(`/api/v1/admin/applications/${application.id}/keys/revoked`, {
        method: "DELETE",
      });
      loadData();
    } catch (cause) {
      alert(cause instanceof Error ? cause.message : t("common.error"));
    }
  };

  const handleRegenerateKey = async (keyId: string) => {
    if (!window.confirm(t("apps.regenerateConfirm"))) return;
    try {
      const res = await api<{ key: string }>(`/api/v1/admin/applications/${application.id}/keys/${keyId}/regenerate`, {
        method: "POST",
      });
      setRevealedKey(res.key);
      loadData();
    } catch (cause) {
      alert(cause instanceof Error ? cause.message : t("common.error"));
    }
  };

  const handleAddMember = async (e: FormEvent) => {
    e.preventDefault();
    if (!selectedUserId) return;
    try {
      await api(`/api/v1/admin/applications/${application.id}/members`, {
        method: "POST",
        body: JSON.stringify({
          userId: selectedUserId,
          role: selectedRole,
        }),
      });
      setSelectedUserId("");
      loadData();
    } catch (cause) {
      alert(cause instanceof Error ? cause.message : t("common.error"));
    }
  };

  const handleRevokeMember = async (userId: string) => {
    try {
      await api(`/api/v1/admin/applications/${application.id}/members/${userId}`, {
        method: "DELETE",
      });
      loadData();
    } catch (cause) {
      alert(cause instanceof Error ? cause.message : t("common.error"));
    }
  };

  const handleUpdateSettings = async (e: FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    const fd = new FormData(e.currentTarget);
    try {
      await api(`/api/v1/admin/applications/${application.id}`, {
        method: "PATCH",
        body: JSON.stringify({
          name: String(fd.get("name") || application.name),
          slug: String(fd.get("slug") || application.slug),
          retentionDays: Number(fd.get("retentionDays") || application.retentionDays),
          isPublic,
          description: description || null,
          githubUrl: githubUrl || null,
          websiteUrl: websiteUrl || null,
          customHeader: customHeader || null,
        }),
      });
      alert(t("apps.saveSettings") + " ✓");
      onClose();
    } catch (cause) {
      alert(cause instanceof Error ? cause.message : t("common.error"));
    }
  };

  const handleDeleteApp = async () => {
    if (!window.confirm(t("apps.deleteConfirm"))) return;
    try {
      await api(`/api/v1/admin/applications/${application.id}`, { method: "DELETE" });
      onClose();
    } catch (cause) {
      alert(cause instanceof Error ? cause.message : t("common.error"));
    }
  };

  const activeKeyHeaderValue = revealedKey
    ? revealedKey
    : activeKey
    ? `${activeKey.keyPrefix}••••••••••••••••••••••••••••••••`
    : "YOUR_SONDE_API_KEY";

    const getSnippet = () => {
    const origin = typeof window !== "undefined" ? window.location.origin : "https://telemetry.yourdomain.com";
    const userAgentVal = customUserAgent.trim() || `${application.slug}/1.0.0 (Client)`;

    switch (codeSnippetLang) {
      case "rs":
        return useEphemeralTokenOption ? `// Cargo.toml 依赖: reqwest, serde, serde_json, tokio, chrono
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Deserialize)]
struct IngestTokenResponse {
    token: String,
    #[serde(rename = "expiresAt")]
    expires_at: i64,
    #[serde(rename = "signingKey")]
    signing_key: String,
}

pub struct SondeClient {
    client: Client,
    endpoint: String,
    api_key: String,
    device_id: String,
    app_version: String,
    disable_logs: bool,
    token_cache: Arc<RwLock<Option<(String, String, i64)>>>,
}

impl SondeClient {
    pub fn new(api_key: &str, device_id: &str, app_version: &str) -> Self {
        let client = Client::builder()
            .user_agent("${userAgentVal}")
            .build()
            .unwrap_or_else(|_| Client::new());
        Self {
            client,
            endpoint: "${origin}/api/v1/ingest".into(),
            api_key: api_key.into(),
            device_id: device_id.into(),
            app_version: app_version.into(),
            disable_logs: ${disableLogsOption},
            token_cache: Arc::new(RwLock::new(None)),
        }
    }

    /// 1. 自动获取并维护 2 分钟短效 Token (支持提前 10s 刷新与 IP 防爆破)
    async fn get_valid_token(&self) -> Result<(String, String), reqwest::Error> {
        let now_ms = chrono::Utc::now().timestamp_millis();
        {
            let guard = self.token_cache.read().await;
            if let Some((ref tok, ref sig_key, exp)) = *guard {
                if exp - now_ms > 10_000 {
                    return Ok((tok.clone(), sig_key.clone()));
                }
            }
        }

        let resp: IngestTokenResponse = self.client
            .post(format!("{}/token", self.endpoint))
            .header("x-sonde-key", &self.api_key)
            .json(&json!({
                "deviceId": self.device_id,
                "appVersion": self.app_version,
                "os": std::env::consts::OS
            }))
            .send()
            .await?
            .json()
            .await?;

        let mut guard = self.token_cache.write().await;
        *guard = Some((resp.token.clone(), resp.signing_key.clone(), resp.expires_at));
        Ok((resp.token, resp.signing_key))
    }

    /// 2. 上报事件 (包含匿名设备 ID、版本与日活属性)
    pub async fn track_event(&self, name: &str, attrs: Option<serde_json::Value>) -> Result<(), reqwest::Error> {
        let (token, _) = self.get_valid_token().await?;
        let payload = json!({
            "items": [{
                "name": name,
                "anonymousId": self.device_id,
                "appVersion": self.app_version,
                "os": std::env::consts::OS,
                "timestamp": chrono::Utc::now().timestamp_millis(),
                "attributes": attrs.unwrap_or(json!({}))
            }]
        });

        self.client
            .post(format!("{}/events", self.endpoint))
            .header("Authorization", format!("Bearer {}", token))
            .json(&payload)
            .send()
            .await?;
        Ok(())
    }

    /// 3. 上报性能与监控指标 (Metrics)
    pub async fn record_metric(&self, name: &str, metric_type: &str, value: f64, unit: &str) -> Result<(), reqwest::Error> {
        let (token, _) = self.get_valid_token().await?;
        let payload = json!({
            "items": [{
                "name": name,
                "metricType": metric_type,
                "value": value,
                "unit": unit,
                "timestamp": chrono::Utc::now().timestamp_millis()
            }]
        });

        self.client
            .post(format!("{}/metrics", self.endpoint))
            .header("Authorization", format!("Bearer {}", token))
            .json(&payload)
            .send()
            .await?;
        Ok(())
    }
${captureErrorsOption ? `
    /// 4. 规范化错误与未捕获崩溃上报
    pub async fn report_error(&self, name: &str, message: &str, stack: Option<&str>, handled: bool) -> Result<(), reqwest::Error> {
        let (token, _) = self.get_valid_token().await?;
        let payload = json!({
            "items": [{
                "name": name,
                "message": message,
                "stackTrace": stack,
                "severity": if handled { "error" } else { "fatal" },
                "handled": handled,
                "anonymousId": self.device_id,
                "appVersion": self.app_version,
                "os": std::env::consts::OS,
                "timestamp": chrono::Utc::now().timestamp_millis()
            }]
        });

        self.client
            .post(format!("{}/errors", self.endpoint))
            .header("Authorization", format!("Bearer {}", token))
            .json(&payload)
            .send()
            .await?;
        Ok(())
    }` : ""}${!disableLogsOption ? `
    /// 5. 上报运行日志 (Logs)
    pub async fn log(&self, level: &str, message: &str) -> Result<(), reqwest::Error> {
        if self.disable_logs { return Ok(()); }
        let (token, _) = self.get_valid_token().await?;
        let payload = json!({
            "items": [{
                "level": level,
                "message": message,
                "timestamp": chrono::Utc::now().timestamp_millis()
            }]
        });

        self.client
            .post(format!("{}/logs", self.endpoint))
            .header("Authorization", format!("Bearer {}", token))
            .json(&payload)
            .send()
            .await?;
        Ok(())
    }` : ""}
}

#[tokio::main]
async fn main() {
    let client = SondeClient::new("${activeKeyHeaderValue}", "device-uuid-9921", "2.4.0");
    client.track_event("user_login", Some(json!({"channel": "official"}))).await.unwrap();
    client.record_metric("cpu_usage_pct", "gauge", 24.5, "%").await.unwrap();
}` : `// 直接使用 Ingest Key 上报 (适用于受信任服务端 / 内部脚本)
use reqwest::Client;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), reqwest::Error> {
    let client = Client::builder().user_agent("${userAgentVal}").build()?;
    client.post("${origin}/api/v1/ingest/events")
        .header("x-sonde-key", "${activeKeyHeaderValue}")
        .json(&json!({
            "items": [{
                "name": "worker_ping",
                "anonymousId": "node-01",
                "appVersion": "1.0.0",
                "timestamp": chrono::Utc::now().timestamp_millis()
            }]
        }))
        .send()
        .await?;
    Ok(())
}`;

      case "ts":
        return `// npm install axios
import axios, { AxiosInstance } from "axios";

export interface TelemetryAttributes {
  [key: string]: string | number | boolean | null;
}

export class SondeTelemetry {
  private endpoint = "${origin}/api/v1/ingest";
  private apiKey = "${activeKeyHeaderValue}";
  private deviceId: string;
  private appVersion: string;
  private userAgent = "${userAgentVal}";
  private disableLogs = ${disableLogsOption};
  private tokenCache?: { token: string; signingKey: string; expiresAt: number };
  private http: AxiosInstance;

  constructor(deviceId?: string, appVersion: string = "1.0.0") {
    this.deviceId = deviceId || "web-" + Math.random().toString(36).slice(2);
    this.appVersion = appVersion;
    this.http = axios.create({
      headers: { "User-Agent": this.userAgent },
      timeout: 5000,
    });
${captureErrorsOption ? `    if (typeof window !== "undefined") {
      window.addEventListener("error", (e) => {
        this.reportError(e.error?.name || "UncaughtError", e.message, e.error?.stack, false);
      });
      window.addEventListener("unhandledrejection", (e) => {
        this.reportError("UnhandledRejection", String(e.reason), e.reason?.stack, false);
      });
    }` : ""}
  }

  // 1. 获取并自动续订 2 分钟短效 Token
  private async getToken(): Promise<string> {
    const now = Date.now();
    if (this.tokenCache && this.tokenCache.expiresAt - now > 10000) {
      return this.tokenCache.token;
    }
    const res = await this.http.post(\`\${this.endpoint}/token\`, {
      deviceId: this.deviceId,
      appVersion: this.appVersion,
      os: typeof navigator !== "undefined" ? navigator.userAgent : "Node.js",
    }, { headers: { "x-sonde-key": this.apiKey } });

    this.tokenCache = {
      token: res.data.token,
      signingKey: res.data.signingKey,
      expiresAt: res.data.expiresAt,
    };
    return this.tokenCache.token;
  }

  // 2. 上报事件与日活 (Events)
  async track(name: string, attributes: TelemetryAttributes = {}) {
    const token = await this.getToken();
    await this.http.post(\`\${this.endpoint}/events\`, {
      items: [{
        name,
        anonymousId: this.deviceId,
        appVersion: this.appVersion,
        os: typeof navigator !== "undefined" ? navigator.platform : "Server",
        timestamp: Date.now(),
        attributes,
      }],
    }, { headers: { Authorization: \`Bearer \${token}\` } });
  }

  // 3. 上报性能与监控指标 (Metrics)
  async metric(name: string, value: number, metricType: "gauge" | "counter" | "histogram" = "gauge", unit: string = "ms") {
    const token = await this.getToken();
    await this.http.post(\`\${this.endpoint}/metrics\`, {
      items: [{
        name,
        metricType,
        value,
        unit,
        timestamp: Date.now(),
      }],
    }, { headers: { Authorization: \`Bearer \${token}\` } });
  }
${captureErrorsOption ? `  // 4. 规范化崩溃与错误上报 (Errors)
  async reportError(name: string, message: string, stackTrace?: string, handled: boolean = true) {
    const token = await this.getToken();
    await this.http.post(\`\${this.endpoint}/errors\`, {
      items: [{
        name,
        message,
        stackTrace,
        severity: handled ? "error" : "fatal",
        handled,
        anonymousId: this.deviceId,
        appVersion: this.appVersion,
        timestamp: Date.now(),
      }],
    }, { headers: { Authorization: \`Bearer \${token}\` } });
  }` : ""}${!disableLogsOption ? `  // 5. 业务运行日志 (Logs)
  async log(level: "debug" | "info" | "warn" | "error" | "fatal", message: string) {
    if (this.disableLogs) return;
    const token = await this.getToken();
    await this.http.post(\`\${this.endpoint}/logs\`, {
      items: [{ level, message, timestamp: Date.now() }],
    }, { headers: { Authorization: \`Bearer \${token}\` } });
  }` : ""}
}

const sonde = new SondeTelemetry();
sonde.track("page_view", { path: "/dashboard", theme: "dark" });`;

      case "go":
        return `package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"net/http"
	"sync"
	"time"
)

type TokenResponse struct {
	Token      string \`json:"token"\`
	ExpiresAt  int64  \`json:"expiresAt"\`
	SigningKey string \`json:"signingKey"\`
}

type SondeClient struct {
	Endpoint    string
	APIKey      string
	DeviceID    string
	AppVersion  string
	UserAgent   string
	DisableLogs bool
	HTTPClient  *http.Client
	mu          sync.RWMutex
	token       string
	expiresAt   int64
}

func NewSondeClient(apiKey, deviceID, version string) *SondeClient {
	return &SondeClient{
		Endpoint:    "${origin}/api/v1/ingest",
		APIKey:      apiKey,
		DeviceID:    deviceID,
		AppVersion:  version,
		UserAgent:   "${userAgentVal}",
		DisableLogs: ${disableLogsOption},
		HTTPClient:  &http.Client{Timeout: 5 * time.Second},
	}
}

func (s *SondeClient) GetToken() (string, error) {
	now := time.Now().UnixMilli()
	s.mu.RLock()
	if s.token != "" && s.expiresAt-now > 10000 {
		defer s.mu.RUnlock()
		return s.token, nil
	}
	s.mu.RUnlock()

	s.mu.Lock()
	defer s.mu.Unlock()

	payload, _ := json.Marshal(map[string]string{
		"deviceId":   s.DeviceID,
		"appVersion": s.AppVersion,
		"os":         "Linux/Windows",
	})
	req, _ := http.NewRequest("POST", s.Endpoint+"/token", bytes.NewBuffer(payload))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("x-sonde-key", s.APIKey)
	req.Header.Set("User-Agent", s.UserAgent)

	resp, err := s.HTTPClient.Do(req)
	if err != nil { return "", err }
	defer resp.Body.Close()

	var tResp TokenResponse
	if err := json.NewDecoder(resp.Body).Decode(&tResp); err != nil {
		return "", err
	}
	s.token = tResp.Token
	s.expiresAt = tResp.ExpiresAt
	return s.token, nil
}

func (s *SondeClient) Track(name string, attrs map[string]interface{}) error {
	tok, err := s.GetToken()
	if err != nil { return err }

	body, _ := json.Marshal(map[string]interface{}{
		"items": []map[string]interface{}{
			{
				"name":        name,
				"anonymousId": s.DeviceID,
				"appVersion":  s.AppVersion,
				"timestamp":   time.Now().UnixMilli(),
				"attributes":  attrs,
			},
		},
	})
	req, _ := http.NewRequest("POST", s.Endpoint+"/events", bytes.NewBuffer(body))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer "+tok)
	req.Header.Set("User-Agent", s.UserAgent)

	resp, err := s.HTTPClient.Do(req)
	if err != nil { return err }
	defer resp.Body.Close()
	return nil
}

func main() {
	client := NewSondeClient("${activeKeyHeaderValue}", "go-worker-01", "1.0.0")
	_ = client.Track("server_start", map[string]interface{}{"port": 8080})
}`;

      case "py":
        return `import requests
import time
import sys
import traceback

class SondeClient:
    def __init__(self, api_key: str = "${activeKeyHeaderValue}", device_id: str = "py-dev-001", app_version: str = "2.4.0"):
        self.endpoint = "${origin}/api/v1/ingest"
        self.api_key = api_key
        self.device_id = device_id
        self.app_version = app_version
        self.user_agent = "${userAgentVal}"
        self.disable_logs = ${disableLogsOption ? "True" : "False"}
        self.token = None
        self.expires_at = 0
        self.session = requests.Session()
        self.session.headers.update({"User-Agent": self.user_agent, "Content-Type": "application/json"})
${captureErrorsOption ? `        sys.excepthook = self._handle_uncaught_exception` : ""}

    def get_token(self) -> str:
        now = int(time.time() * 1000)
        if self.token and (self.expires_at - now > 10000):
            return self.token
        res = self.session.post(
            f"{self.endpoint}/token",
            json={"deviceId": self.device_id, "appVersion": self.app_version},
            headers={"x-sonde-key": self.api_key}
        ).json()
        self.token = res["token"]
        self.expires_at = res["expiresAt"]
        return self.token

    def track(self, name: str, attributes: dict = None):
        tok = self.get_token()
        payload = {
            "items": [{
                "name": name,
                "anonymousId": self.device_id,
                "appVersion": self.app_version,
                "timestamp": int(time.time() * 1000),
                "attributes": attributes or {}
            }]
        }
        self.session.post(f"{self.endpoint}/events", json=payload, headers={"Authorization": f"Bearer {tok}"})

    def metric(self, name: str, value: float, metric_type: str = "gauge", unit: str = "ms"):
        tok = self.get_token()
        payload = {"items": [{"name": name, "metricType": metric_type, "value": value, "unit": unit, "timestamp": int(time.time() * 1000)}]}
        self.session.post(f"{self.endpoint}/metrics", json=payload, headers={"Authorization": f"Bearer {tok}"})
${captureErrorsOption ? `    def report_error(self, name: str, message: str, stack_trace: str = None, handled: bool = True):
        tok = self.get_token()
        payload = {
            "items": [{
                "name": name,
                "message": message,
                "stackTrace": stack_trace,
                "severity": "error" if handled else "fatal",
                "handled": handled,
                "anonymousId": self.device_id,
                "appVersion": self.app_version,
                "timestamp": int(time.time() * 1000)
            }]
        }
        self.session.post(f"{self.endpoint}/errors", json=payload, headers={"Authorization": f"Bearer {tok}"})

    def _handle_uncaught_exception(self, exc_type, exc_value, exc_traceback):
        tb_str = "".join(traceback.format_exception(exc_type, exc_value, exc_traceback))
        self.report_error(exc_type.__name__, str(exc_value), tb_str, handled=False)
        sys.__excepthook__(exc_type, exc_value, exc_traceback)` : ""}${!disableLogsOption ? `    def log(self, level: str, message: str):
        if self.disable_logs: return
        tok = self.get_token()
        self.session.post(f"{self.endpoint}/logs", json={"items": [{"level": level, "message": message, "timestamp": int(time.time() * 1000)}]}, headers={"Authorization": f"Bearer {tok}"})` : ""}

sonde = SondeClient()
sonde.track("app_boot", {"channel": "official"});`;

      case "csharp":
        return `using System;
using System.Net.Http;
using System.Text;
using System.Text.Json;
using System.Threading.Tasks;

public class SondeClient
{
    private readonly HttpClient _http = new HttpClient();
    private readonly string _endpoint = "${origin}/api/v1/ingest";
    private readonly string _apiKey = "${activeKeyHeaderValue}";
    private readonly string _deviceId;
    private readonly string _appVersion;
    private string _token;
    private long _expiresAt;

    public SondeClient(string deviceId = "csharp-client-1", string appVersion = "1.0.0")
    {
        _deviceId = deviceId;
        _appVersion = appVersion;
        _http.DefaultRequestHeaders.UserAgent.ParseAdd("${userAgentVal}");
${captureErrorsOption ? `        AppDomain.CurrentDomain.UnhandledException += (s, e) => {
            if (e.ExceptionObject is Exception ex)
                ReportErrorAsync(ex.GetType().Name, ex.Message, ex.StackTrace, false).Wait();
        };` : ""}
    }

    public async Task<string> GetTokenAsync()
    {
        var now = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
        if (!string.IsNullOrEmpty(_token) && (_expiresAt - now > 10000)) return _token;

        var json = JsonSerializer.Serialize(new { deviceId = _deviceId, appVersion = _appVersion });
        var req = new HttpRequestMessage(HttpMethod.Post, _endpoint + "/token") {
            Content = new StringContent(json, Encoding.UTF8, "application/json")
        };
        req.Headers.Add("x-sonde-key", _apiKey);
        var resp = await _http.SendAsync(req);
        var doc = JsonDocument.Parse(await resp.Content.ReadAsStringAsync());
        _token = doc.RootElement.GetProperty("token").GetString();
        _expiresAt = doc.RootElement.GetProperty("expiresAt").GetInt64();
        return _token;
    }

    public async Task TrackEventAsync(string name, object attributes = null)
    {
        var token = await GetTokenAsync();
        var payload = new {
            items = new[] {
                new { name, anonymousId = _deviceId, appVersion = _appVersion, timestamp = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds(), attributes }
            }
        };
        var req = new HttpRequestMessage(HttpMethod.Post, _endpoint + "/events") {
            Content = new StringContent(JsonSerializer.Serialize(payload), Encoding.UTF8, "application/json")
        };
        req.Headers.Add("Authorization", "Bearer " + token);
        await _http.SendAsync(req);
    }
${captureErrorsOption ? `    public async Task ReportErrorAsync(string name, string message, string stackTrace = null, bool handled = true)
    {
        var token = await GetTokenAsync();
        var payload = new {
            items = new[] {
                new { name, message, stackTrace, severity = handled ? "error" : "fatal", handled, anonymousId = _deviceId, appVersion = _appVersion, timestamp = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds() }
            }
        };
        var req = new HttpRequestMessage(HttpMethod.Post, _endpoint + "/errors") {
            Content = new StringContent(JsonSerializer.Serialize(payload), Encoding.UTF8, "application/json")
        };
        req.Headers.Add("Authorization", "Bearer " + token);
        await _http.SendAsync(req);
    }` : ""}
}`;

      case "cpp":
        return `// C++ 17/20 (Libcurl)
#include <iostream>
#include <string>
#include <curl/curl.h>

// Step 1: 申请 2 分钟临时 Token
// POST ${origin}/api/v1/ingest/token
// Header: x-sonde-key: ${activeKeyHeaderValue}
// Header: User-Agent: ${userAgentVal}
// Body:   {"deviceId": "cpp_client_001", "appVersion": "1.0.0"}

// Step 2: 携带 Bearer Token 上报事件
// POST ${origin}/api/v1/ingest/events
// Header: Authorization: Bearer <TOKEN>
// Header: User-Agent: ${userAgentVal}
// Body:   {"items": [{"name": "app_launch", "anonymousId": "cpp_client_001"}]}`;

      case "curl":
      default:
        return `# ==========================================================
# 步骤 1: 客户端申请 2 分钟短效 Token (含 UA 校验与 IP 限流保护)
# ==========================================================
TOKEN=$(curl -s -X POST "${origin}/api/v1/ingest/token" \\
  -H "Content-Type: application/json" \\
  -H "User-Agent: ${userAgentVal}" \\
  -H "x-sonde-key: ${activeKeyHeaderValue}" \\
  -d '{ "deviceId": "device_uuid_9921", "appVersion": "2.4.0", "os": "Windows 11" }' | jq -r .token)

echo "获取到的 2 分钟短效 Token: $TOKEN"

# ==========================================================
# 步骤 2: 携带短效 Token 上报事件与日活 (Events)
# ==========================================================
curl -X POST "${origin}/api/v1/ingest/events" \\
  -H "Content-Type: application/json" \\
  -H "User-Agent: ${userAgentVal}" \\
  -H "Authorization: Bearer $TOKEN" \\
  -d '{
    "items": [
      {
        "name": "login_success",
        "anonymousId": "device_uuid_9921",
        "appVersion": "2.4.0",
        "timestamp": $(date +%s%3N),
        "attributes": { "channel": "official" }
      }
    ]
  }'

# ==========================================================
# 步骤 3: 性能与指标上报 (Metrics)
# ==========================================================
curl -X POST "${origin}/api/v1/ingest/metrics" \\
  -H "Content-Type: application/json" \\
  -H "User-Agent: ${userAgentVal}" \\
  -H "Authorization: Bearer $TOKEN" \\
  -d '{
    "items": [
      {
        "name": "http_request_duration_ms",
        "metricType": "gauge",
        "value": 18.5,
        "unit": "ms",
        "timestamp": $(date +%s%3N)
      }
    ]
  }'

# ==========================================================
# 步骤 4: 规范化崩溃与错误信号上报 (Errors)
# ==========================================================
curl -X POST "${origin}/api/v1/ingest/errors" \\
  -H "Content-Type: application/json" \\
  -H "User-Agent: ${userAgentVal}" \\
  -H "Authorization: Bearer $TOKEN" \\
  -d '{
    "items": [
      {
        "name": "NullReferenceException",
        "message": "Object reference not set to an instance of an object",
        "stackTrace": "at App.MainModule.Execute() in MainModule.cs:line 42",
        "severity": "fatal",
        "handled": false,
        "anonymousId": "device_uuid_9921",
        "timestamp": $(date +%s%3N)
      }
    ]
  }'`;
    }
  };

  return (
    <Modal
      size="lg"
      title={application.name}
      subtitle={<code>{application.slug}</code>}
      icon={<AppWindow size={20} />}
      onClose={onClose}
      actions={
        <button
          type="button"
          className="secondary-button compact"
          onClick={onExport}
          title={t("apps.exportApp")}
          style={{ gap: "6px" }}
        >
          <Download size={14} />
          {t("apps.exportApp")}
        </button>
      }
      tabs={
        <>
          <button
            type="button"
            className={`sonde-modal-tab ${activeTab === "integration" ? "active" : ""}`}
            onClick={() => setActiveTab("integration")}
          >
            <Activity size={15} />
            {t("apps.tabIntegration")}
          </button>
          <button
            type="button"
            className={`sonde-modal-tab ${activeTab === "keys" ? "active" : ""}`}
            onClick={() => setActiveTab("keys")}
          >
            <KeyRound size={15} />
            {t("apps.tabKeys")} ({apiKeys.length})
          </button>
          <button
            type="button"
            className={`sonde-modal-tab ${activeTab === "members" ? "active" : ""}`}
            onClick={() => setActiveTab("members")}
          >
            <Users size={15} />
            {t("apps.tabMembers")} ({members.length})
          </button>
          <button
            type="button"
            className={`sonde-modal-tab ${activeTab === "public" ? "active" : ""}`}
            onClick={() => setActiveTab("public")}
          >
            <Globe size={15} />
            {t("apps.tabPublic")}
          </button>
          <button
            type="button"
            className={`sonde-modal-tab ${activeTab === "settings" ? "active" : ""}`}
            onClick={() => setActiveTab("settings")}
          >
            <Settings size={15} />
            {t("apps.tabSettings")}
          </button>
        </>
      }
    >
      {activeTab === "integration" ? (
        <div className="space-y-4">
          {/* Endpoint Box */}
          <div className="p-4 rounded-2xl bg-[var(--input-bg)] border border-[var(--border-soft)] space-y-3">
            <div className="flex items-center justify-between">
              <span className="text-xs font-bold text-[var(--text)] uppercase tracking-wider">遥测数据管道端点 (Endpoints)</span>
              {activeKey ? (
                <span className="text-xs text-[var(--muted)]">
                  当前接入密钥: <code className="text-[var(--signal)] font-bold">{activeKey.name}</code> ({activeKey.environmentName})
                </span>
              ) : null}
            </div>

            <div className="grid grid-cols-1 md:grid-cols-2 gap-3 text-xs font-mono">
              <div className="flex items-center justify-between p-2.5 rounded-xl bg-[var(--panel-strong)] border border-[var(--border-soft)]">
                <div className="truncate mr-2">
                  <span className="text-[var(--muted)] text-[10px] block font-sans">全量数据上报基础路径:</span>
                  <span className="text-[var(--text)] font-bold">{ingestUrl}</span>
                </div>
                <button
                  type="button"
                  className="p-1.5 rounded-lg text-[var(--muted)] hover:text-[var(--text)] hover:bg-[var(--input-bg)] cursor-pointer flex-shrink-0"
                  onClick={() => navigator.clipboard.writeText(ingestUrl)}
                  title={t("common.copy")}
                >
                  <Copy size={13} />
                </button>
              </div>

              <div className="flex items-center justify-between p-2.5 rounded-xl bg-[var(--panel-strong)] border border-[var(--border-soft)]">
                <div className="truncate mr-2">
                  <span className="text-[var(--muted)] text-[10px] block font-sans">事件与日活上报路径:</span>
                  <span className="text-[var(--signal)] font-bold">{eventsUrl}</span>
                </div>
                <button
                  type="button"
                  className="p-1.5 rounded-lg text-[var(--muted)] hover:text-[var(--text)] hover:bg-[var(--input-bg)] cursor-pointer flex-shrink-0"
                  onClick={() => navigator.clipboard.writeText(eventsUrl)}
                  title={t("common.copy")}
                >
                  <Copy size={13} />
                </button>
              </div>
            </div>
          </div>

          {/* Dynamic Feature Config Toggles */}
          <div className="p-3.5 rounded-2xl bg-[var(--panel-strong)] border border-[var(--border-soft)] flex flex-wrap items-center justify-between gap-4 text-xs">
            <div className="flex items-center gap-2 font-bold text-[var(--text)]">
              <Sliders size={14} className="text-[var(--signal)]" />
              <span>实时代码生成选项</span>
            </div>

            <div className="w-full flex flex-col gap-2.5">
              <div className="flex items-center gap-4 flex-wrap">
                <label className="inline-flex items-center gap-1.5 cursor-pointer select-none text-[var(--text)] font-medium">
                  <input
                    type="checkbox"
                    checked={useEphemeralTokenOption}
                    onChange={(e) => setUseEphemeralTokenOption(e.target.checked)}
                    className="rounded text-[var(--signal)] focus:ring-0"
                  />
                  <span className={useEphemeralTokenOption ? "text-[var(--signal)] font-bold" : ""}>
                    🛡️ 2 步短效 Token 与 HMAC 签名防刷 (推荐客户端开发)
                  </span>
                </label>

                <label className="inline-flex items-center gap-1.5 cursor-pointer select-none text-[var(--text)] font-medium">
                  <input
                    type="checkbox"
                    checked={captureErrorsOption}
                    onChange={(e) => setCaptureErrorsOption(e.target.checked)}
                    className="rounded text-[var(--signal)] focus:ring-0"
                  />
                  <span>捕获未处理崩溃与异常</span>
                </label>

                <label className="inline-flex items-center gap-1.5 cursor-pointer select-none text-[var(--text)] font-medium">
                  <input
                    type="checkbox"
                    checked={disableLogsOption}
                    onChange={(e) => setDisableLogsOption(e.target.checked)}
                    className="rounded text-[var(--signal)] focus:ring-0"
                  />
                  <span className={disableLogsOption ? "text-[var(--amber)] font-bold" : ""}>
                    关闭日志上传 (disable_logs)
                  </span>
                </label>
              </div>

              {/* Custom User-Agent Input */}
              <div className="flex items-center gap-2 pt-2 border-t border-[var(--border-soft)]">
                <span className="text-[11px] text-[var(--muted)] whitespace-nowrap font-medium">🛡️ 客户端 User-Agent:</span>
                <input
                  type="text"
                  value={customUserAgent}
                  onChange={(e) => setCustomUserAgent(e.target.value)}
                  placeholder={`${application.slug}/1.0.0 (Client)`}
                  className="flex-1 px-2.5 py-1 rounded-lg text-xs font-mono bg-[var(--input-bg)] border border-[var(--border-soft)] text-[var(--text)] focus:outline-none focus:border-[var(--signal)]"
                />
                <span className="text-[10px] text-[var(--muted)] hidden sm:inline">(Token 申请与上报必须携带有效 UA 校验防刷)</span>
              </div>
            </div>
          </div>

          {/* Language Tabs with Official SVG Icons */}
          <div className="flex items-center gap-1.5 overflow-x-auto pb-1 scrollbar-none border-b border-[var(--border-soft)]">
            <button
              type="button"
              className={`px-3 py-1.5 rounded-xl text-xs font-bold transition-all cursor-pointer inline-flex items-center gap-2 ${
                codeSnippetLang === "rs"
                  ? "bg-[var(--signal)] text-white shadow-xs"
                  : "text-[var(--muted)] hover:text-[var(--text)] hover:bg-[var(--input-bg)]"
              }`}
              onClick={() => setCodeSnippetLang("rs")}
            >
              <SiRust size={14} className={codeSnippetLang === "rs" ? "text-white" : "text-[#f74c00]"} />
              <span>Rust</span>
            </button>

            <button
              type="button"
              className={`px-3 py-1.5 rounded-xl text-xs font-bold transition-all cursor-pointer inline-flex items-center gap-2 ${
                codeSnippetLang === "ts"
                  ? "bg-[var(--signal)] text-white shadow-xs"
                  : "text-[var(--muted)] hover:text-[var(--text)] hover:bg-[var(--input-bg)]"
              }`}
              onClick={() => setCodeSnippetLang("ts")}
            >
              <SiTypescript size={14} className={codeSnippetLang === "ts" ? "text-white" : "text-[#3178c6]"} />
              <span>TypeScript / JS</span>
            </button>

            <button
              type="button"
              className={`px-3 py-1.5 rounded-xl text-xs font-bold transition-all cursor-pointer inline-flex items-center gap-2 ${
                codeSnippetLang === "go"
                  ? "bg-[var(--signal)] text-white shadow-xs"
                  : "text-[var(--muted)] hover:text-[var(--text)] hover:bg-[var(--input-bg)]"
              }`}
              onClick={() => setCodeSnippetLang("go")}
            >
              <SiGo size={15} className={codeSnippetLang === "go" ? "text-white" : "text-[#00add8]"} />
              <span>Go</span>
            </button>

            <button
              type="button"
              className={`px-3 py-1.5 rounded-xl text-xs font-bold transition-all cursor-pointer inline-flex items-center gap-2 ${
                codeSnippetLang === "py"
                  ? "bg-[var(--signal)] text-white shadow-xs"
                  : "text-[var(--muted)] hover:text-[var(--text)] hover:bg-[var(--input-bg)]"
              }`}
              onClick={() => setCodeSnippetLang("py")}
            >
              <SiPython size={14} className={codeSnippetLang === "py" ? "text-white" : "text-[#3776ab]"} />
              <span>Python</span>
            </button>

            <button
              type="button"
              className={`px-3 py-1.5 rounded-xl text-xs font-bold transition-all cursor-pointer inline-flex items-center gap-2 ${
                codeSnippetLang === "csharp"
                  ? "bg-[var(--signal)] text-white shadow-xs"
                  : "text-[var(--muted)] hover:text-[var(--text)] hover:bg-[var(--input-bg)]"
              }`}
              onClick={() => setCodeSnippetLang("csharp")}
            >
              <SiDotnet size={14} className={codeSnippetLang === "csharp" ? "text-white" : "text-[#512bd4]"} />
              <span>C# / .NET</span>
            </button>

            <button
              type="button"
              className={`px-3 py-1.5 rounded-xl text-xs font-bold transition-all cursor-pointer inline-flex items-center gap-2 ${
                codeSnippetLang === "cpp"
                  ? "bg-[var(--signal)] text-white shadow-xs"
                  : "text-[var(--muted)] hover:text-[var(--text)] hover:bg-[var(--input-bg)]"
              }`}
              onClick={() => setCodeSnippetLang("cpp")}
            >
              <SiCplusplus size={14} className={codeSnippetLang === "cpp" ? "text-white" : "text-[#00599c]"} />
              <span>C++</span>
            </button>

            <button
              type="button"
              className={`px-3 py-1.5 rounded-xl text-xs font-bold transition-all cursor-pointer inline-flex items-center gap-2 ${
                codeSnippetLang === "curl"
                  ? "bg-[var(--signal)] text-white shadow-xs"
                  : "text-[var(--muted)] hover:text-[var(--text)] hover:bg-[var(--input-bg)]"
              }`}
              onClick={() => setCodeSnippetLang("curl")}
            >
              <SiCurl size={14} className={codeSnippetLang === "curl" ? "text-white" : "text-[#00d1b2]"} />
              <span>cURL / REST</span>
            </button>
          </div>

          {/* Code Snippet Box */}
          <div className="relative rounded-2xl bg-[#0d1117] border border-[var(--border)] overflow-hidden shadow-xl">
            <div className="flex items-center justify-between px-4 py-2 bg-[#161b22] border-b border-[#30363d] text-xs">
              <span className="font-mono text-gray-400 font-semibold uppercase">{codeSnippetLang} SDK Integration</span>
              <button
                type="button"
                onClick={() => {
                  navigator.clipboard.writeText(getSnippet());
                  setSnippetCopied(true);
                  setTimeout(() => setSnippetCopied(false), 2000);
                }}
                className="inline-flex items-center gap-1.5 px-3 py-1 rounded-lg bg-[var(--signal)] text-white font-bold hover:brightness-110 active:scale-95 transition-all cursor-pointer shadow-xs"
              >
                {snippetCopied ? <Check size={13} /> : <Copy size={13} />}
                <span>{snippetCopied ? "已复制到剪贴板" : "复制代码"}</span>
              </button>
            </div>
            <pre className="p-4 text-xs font-mono text-gray-200 overflow-x-auto max-h-[360px] leading-relaxed select-text">
              <code>{getSnippet()}</code>
            </pre>
          </div>

          {/* Telemetry Data Contracts & Security Architecture Guide */}
          <div className="flex flex-col gap-4 mt-2">
            <div className="text-xs font-bold text-[var(--text)] flex items-center gap-2">
              <ShieldCheck size={15} className="text-[var(--signal)]" />
              <span>遥测数据契约与接口规范 (Data Contract Specification)</span>
            </div>

            <div className="grid grid-cols-1 md:grid-cols-2 gap-3 text-xs">
              <div className="p-3.5 rounded-2xl bg-[var(--panel-strong)] border border-[var(--border-soft)] flex flex-col gap-2">
                <div className="flex items-center justify-between">
                  <span className="font-bold text-[var(--text)] flex items-center gap-1.5">
                    <span className="w-2 h-2 rounded-full bg-[var(--signal)]" />
                    1. 事件与日活 (Events)
                  </span>
                  <code className="text-[10px] text-[var(--muted)]">POST /ingest/events</code>
                </div>
                <p className="text-[11px] text-[var(--muted)] leading-relaxed m-0">
                  用于上报应用启动、页面浏览、功能流转与留存分析。DAU / MAU 指标严格依赖 <code>anonymousId</code> 去重计算。
                </p>
                <div className="bg-[var(--input-bg)] p-2 rounded-xl text-[10px] font-mono text-[var(--text)]">
                  name, anonymousId, appVersion, os, timestamp, attributes
                </div>
              </div>

              <div className="p-3.5 rounded-2xl bg-[var(--panel-strong)] border border-[var(--border-soft)] flex flex-col gap-2">
                <div className="flex items-center justify-between">
                  <span className="font-bold text-[var(--text)] flex items-center gap-1.5">
                    <span className="w-2 h-2 rounded-full bg-blue-500" />
                    2. 性能与监控 (Metrics)
                  </span>
                  <code className="text-[10px] text-[var(--muted)]">POST /ingest/metrics</code>
                </div>
                <p className="text-[11px] text-[var(--muted)] leading-relaxed m-0">
                  支持 <code>gauge</code> (仪表值)、<code>counter</code> (计数器) 与 <code>histogram</code> (直方图)，用于 APM 耗时及系统负载。
                </p>
                <div className="bg-[var(--input-bg)] p-2 rounded-xl text-[10px] font-mono text-[var(--text)]">
                  name, metricType, value, unit, timestamp, attributes
                </div>
              </div>

              <div className="p-3.5 rounded-2xl bg-[var(--panel-strong)] border border-[var(--border-soft)] flex flex-col gap-2">
                <div className="flex items-center justify-between">
                  <span className="font-bold text-[var(--text)] flex items-center gap-1.5">
                    <span className="w-2 h-2 rounded-full bg-red-500" />
                    3. 错误与崩溃 (Errors)
                  </span>
                  <code className="text-[10px] text-[var(--muted)]">POST /ingest/errors</code>
                </div>
                <p className="text-[11px] text-[var(--muted)] leading-relaxed m-0">
                  规范化上报未捕获崩溃 (fatal) 与已捕获异常 (error)，携带完整调用堆栈 (stackTrace) 进行根因定位。
                </p>
                <div className="bg-[var(--input-bg)] p-2 rounded-xl text-[10px] font-mono text-[var(--text)]">
                  name, message, stackTrace, severity, handled, timestamp
                </div>
              </div>

              <div className="p-3.5 rounded-2xl bg-[var(--panel-strong)] border border-[var(--border-soft)] flex flex-col gap-2">
                <div className="flex items-center justify-between">
                  <span className="font-bold text-[var(--text)] flex items-center gap-1.5">
                    <span className="w-2 h-2 rounded-full bg-amber-500" />
                    4. 运行日志 (Logs)
                  </span>
                  <code className="text-[10px] text-[var(--muted)]">POST /ingest/logs</code>
                </div>
                <p className="text-[11px] text-[var(--muted)] leading-relaxed m-0">
                  业务运行调试日志。可通过客户端选项 <code>disable_logs</code> 动态关闭日志上报以节约网络开销。
                </p>
                <div className="bg-[var(--input-bg)] p-2 rounded-xl text-[10px] font-mono text-[var(--text)]">
                  level (info/warn/error), message, timestamp, attributes
                </div>
              </div>
            </div>
          </div>
        </div>
      ) : null}

      {activeTab === "keys" ? (
        <>
          {revealedKey ? (
            <div className="key-reveal" role="status">
              <div>
                <KeyRound aria-hidden="true" />
                <div>
                  <strong>{t("apps.newKeyCreated")}</strong>
                  <code>{revealedKey}</code>
                  <small>{t("apps.newKeyCreatedHint")}</small>
                </div>
              </div>
              <button
                type="button"
                onClick={() => navigator.clipboard.writeText(revealedKey)}
                aria-label="Copy key"
              >
                <Copy size={16} />
              </button>
            </div>
          ) : null}

          <div className="keys-header">
            <span className="text-xs font-semibold text-muted uppercase">{t("apps.tabKeys")}</span>
            {!creatingKey ? (
              <button
                type="button"
                className="primary-button compact"
                style={{ minHeight: "36px" }}
                onClick={() => setCreatingKey(true)}
              >
                <Plus size={15} />
                {t("apps.newKey")}
              </button>
            ) : null}
          </div>

          {creatingKey ? (
            <form className="inline-create" onSubmit={handleCreateKey}>
              <label className="field">
                <span>{t("apps.keyName")}</span>
                <input
                  value={newKeyName}
                  onChange={(e) => setNewKeyName(e.target.value)}
                  placeholder="e.g. Staging Server Key"
                  required
                />
              </label>
              <label className="field">
                <span>{t("apps.environment")}</span>
                <select
                  value={newKeyEnvId}
                  onChange={(e) => setNewKeyEnvId(e.target.value)}
                  required
                >
                  {environments.map((env) => (
                    <option key={env.id} value={env.id}>
                      {env.name}
                    </option>
                  ))}
                </select>
              </label>

              <div className="field">
                <span className="text-xs font-semibold text-[var(--text)] mb-1 block">密钥授权权限 (Scopes)</span>
                <div className="grid grid-cols-2 gap-2 p-3 rounded-xl bg-[var(--input-bg)] border border-[var(--border-soft)] text-xs">
                  <label className="flex items-center gap-2 cursor-pointer select-none text-[var(--text)] font-medium">
                    <input
                      type="checkbox"
                      checked={newKeyScopes.includes("telemetry.events")}
                      onChange={(e) => toggleScope("telemetry.events", e.target.checked)}
                      className="rounded text-[var(--signal)] focus:ring-0"
                    />
                    <span>事件与日活 (Events)</span>
                  </label>
                  <label className="flex items-center gap-2 cursor-pointer select-none text-[var(--text)] font-medium">
                    <input
                      type="checkbox"
                      checked={newKeyScopes.includes("telemetry.metrics")}
                      onChange={(e) => toggleScope("telemetry.metrics", e.target.checked)}
                      className="rounded text-[var(--signal)] focus:ring-0"
                    />
                    <span>指标监控 (Metrics)</span>
                  </label>
                  <label className="flex items-center gap-2 cursor-pointer select-none text-[var(--text)] font-medium">
                    <input
                      type="checkbox"
                      checked={newKeyScopes.includes("telemetry.logs")}
                      onChange={(e) => toggleScope("telemetry.logs", e.target.checked)}
                      className="rounded text-[var(--signal)] focus:ring-0"
                    />
                    <span>运行日志 (Logs)</span>
                  </label>
                  <label className="flex items-center gap-2 cursor-pointer select-none text-[var(--text)] font-medium">
                    <input
                      type="checkbox"
                      checked={newKeyScopes.includes("telemetry.errors")}
                      onChange={(e) => toggleScope("telemetry.errors", e.target.checked)}
                      className="rounded text-[var(--signal)] focus:ring-0"
                    />
                    <span>错误与崩溃 (Errors)</span>
                  </label>
                </div>
              </div>
              <div className="flex gap-2">
                <button className="primary-button compact">{t("common.create")}</button>
                <button
                  type="button"
                  className="secondary-button compact"
                  onClick={() => setCreatingKey(false)}
                >
                  {t("common.cancel")}
                </button>
              </div>
            </form>
          ) : null}

          <div style={{ overflowX: "auto" }}>
            <table className="keys-table">
              <thead>
                <tr>
                  <th>{t("apps.keyName")}</th>
                  <th>{t("apps.environment")}</th>
                  <th>{t("apps.keyPrefix")}</th>
                  <th>{t("apps.status")}</th>
                  <th>{t("apps.createdAt")}</th>
                  <th></th>
                </tr>
              </thead>
              <tbody>
                {apiKeys.map((key) => (
                  <tr key={key.id}>
                    <td>
                      <strong>{key.name}</strong>
                    </td>
                    <td>
                      <code>{key.environmentName}</code>
                    </td>
                    <td>
                      <code>{key.keyPrefix}••••</code>
                    </td>
                    <td>
                      {key.isActive ? (
                        <span className="status-ok">
                          <i />
                          {t("apps.active")}
                        </span>
                      ) : (
                        <span style={{ color: "var(--danger)" }}>{t("apps.revoked")}</span>
                      )}
                    </td>
                    <td>{new Date(key.createdAt).toLocaleDateString()}</td>
                    <td>
                      <div className="key-actions">
                        <button
                          type="button"
                          onClick={() => navigator.clipboard.writeText(key.keyPrefix)}
                          title={t("apps.copyPrefix")}
                        >
                          <Copy size={13} />
                        </button>
                        {key.isActive ? (
                          <>
                            <button
                              type="button"
                              onClick={() => handleRegenerateKey(key.id)}
                              title={t("apps.regenerate")}
                            >
                              <RefreshCw size={13} />
                            </button>
                            <button
                              type="button"
                              className="danger"
                              onClick={() => handleRevokeKey(key.id)}
                              title={t("apps.revoke")}
                            >
                              <Trash2 size={13} />
                            </button>
                          </>
                        ) : null}
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </>
      ) : null}

      {activeTab === "members" ? (
        <div className="flex flex-col gap-6">
          <form className="inline-create" onSubmit={handleAddMember}>
            <label className="field">
              <span>{t("access.username")}</span>
              <select
                value={selectedUserId}
                onChange={(e) => setSelectedUserId(e.target.value)}
                required
              >
                <option value="">{t("apps.selectMember")}</option>
                {allUsers.map((u) => (
                  <option key={u.id} value={u.id}>
                    {u.username} ({u.email})
                  </option>
                ))}
              </select>
            </label>
            <label className="field">
              <span>{t("apps.memberRole")}</span>
              <select
                value={selectedRole}
                onChange={(e) => setSelectedRole(e.target.value)}
              >
                <option value="Manager">{t("apps.manager")}</option>
                <option value="Viewer">{t("apps.viewer")}</option>
              </select>
            </label>
            <div>
              <button className="primary-button compact">
                <UserPlus size={15} />
                {t("apps.grantMember")}
              </button>
            </div>
          </form>

          <table className="keys-table">
            <thead>
              <tr>
                <th>{t("access.username")}</th>
                <th>{t("access.email")}</th>
                <th>{t("apps.memberRole")}</th>
                <th>{t("apps.createdAt")}</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {members.map((m) => (
                <tr key={m.userId}>
                  <td>
                    <strong>{m.username}</strong>
                  </td>
                  <td>{m.email}</td>
                  <td>
                    <span className="app-public-badge" style={{ textTransform: "capitalize" }}>
                      {m.role}
                    </span>
                  </td>
                  <td>{new Date(m.grantedAt).toLocaleDateString()}</td>
                  <td>
                    <button
                      type="button"
                      className="secondary-button compact"
                      style={{ color: "var(--danger)", padding: "4px 8px" }}
                      onClick={() => handleRevokeMember(m.userId)}
                      title={t("apps.revokeMember")}
                    >
                      <Trash2 size={13} />
                    </button>
                  </td>
                </tr>
              ))}
              {members.length === 0 ? (
                <tr>
                  <td colSpan={5} className="text-center text-muted" style={{ padding: "24px" }}>
                    {t("apps.noMembersYet")}
                  </td>
                </tr>
              ) : null}
            </tbody>
          </table>
        </div>
      ) : null}

      {activeTab === "public" ? (
        <form className="flex flex-col gap-4" onSubmit={handleUpdateSettings}>
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
              padding: "14px 16px",
              background: "var(--panel-strong)",
              border: "1px solid var(--border)",
              borderRadius: "8px",
            }}
          >
            <div>
              <strong style={{ display: "block", fontSize: "0.9rem" }}>{t("apps.enablePublic")}</strong>
              <span className="text-xs text-muted">
                Make operational status & version distributions publicly visible at `/p/${application.slug}`.
              </span>
            </div>
            <input
              type="checkbox"
              checked={isPublic}
              onChange={(e) => setIsPublic(e.target.checked)}
              style={{ width: "20px", height: "20px", accentColor: "var(--signal)", cursor: "pointer" }}
            />
          </div>

          {isPublic ? (
            <div
              style={{
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                padding: "10px 14px",
                background: "var(--input-bg)",
                border: "1px solid var(--border-soft)",
                borderRadius: "6px",
              }}
            >
              <code className="text-signal">{origin}/p/{application.slug}</code>
              <a
                href={`/p/${application.slug}`}
                target="_blank"
                rel="noreferrer"
                className="secondary-button compact"
                style={{ gap: "6px", color: "var(--signal)" }}
              >
                <ExternalLink size={13} />
                {t("apps.previewPublic")}
              </a>
            </div>
          ) : null}

          <label className="field">
            <span>{t("apps.description")}</span>
            <textarea
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder="A high-performance application built with modern architecture..."
              rows={3}
              style={{
                background: "var(--input-bg)",
                border: "1px solid var(--border-soft)",
                borderRadius: "6px",
                padding: "10px",
                color: "var(--text)",
                fontSize: "0.82rem",
              }}
            />
          </label>

          <label className="field">
            <span>{t("apps.githubUrl")}</span>
            <input
              value={githubUrl}
              onChange={(e) => setGithubUrl(e.target.value)}
              placeholder="https://github.com/organization/repo"
            />
          </label>

          <label className="field">
            <span>{t("apps.websiteUrl")}</span>
            <input
              value={websiteUrl}
              onChange={(e) => setWebsiteUrl(e.target.value)}
              placeholder="https://myapp.example.com"
            />
          </label>

          <label className="field">
            <span>{t("apps.customTagline")}</span>
            <input
              value={customHeader}
              onChange={(e) => setCustomHeader(e.target.value)}
              placeholder="e.g. Official Telemetry Node / v1.0.0"
            />
          </label>

          <div style={{ marginTop: "8px" }}>
            <button className="primary-button">{t("apps.saveSettings")}</button>
          </div>
        </form>
      ) : null}

      {activeTab === "settings" ? (
        <div className="flex flex-col gap-6">
          <form className="flex flex-col gap-4" onSubmit={handleUpdateSettings}>
            <Field
              name="name"
              label="Application Name"
              defaultValue={application.name}
            />
            <Field
              name="slug"
              label="URL Slug"
              pattern="[a-z0-9]+(?:-[a-z0-9]+)*"
              defaultValue={application.slug}
            />
            <label className="field">
              <span>{t("apps.retention")}</span>
              <select name="retentionDays" defaultValue={application.retentionDays}>
                <option value={30}>30 {t("apps.retentionDays")}</option>
                <option value={90}>90 {t("apps.retentionDays")}</option>
                <option value={180}>180 {t("apps.retentionDays")}</option>
                <option value={365}>365 {t("apps.retentionDays")} (1 Year)</option>
                <option value={730}>730 {t("apps.retentionDays")} (2 Years)</option>
                <option value={3650}>3650 {t("apps.retentionDays")} (10 Years)</option>
              </select>
            </label>
            <div style={{ marginTop: 8 }}>
              <button className="primary-button">{t("apps.saveSettings")}</button>
            </div>
          </form>

          <hr style={{ borderColor: "var(--border-soft)", margin: "10px 0" }} />

          <div className="flex flex-col gap-3">
            <span className="text-xs font-semibold text-danger uppercase">{t("apps.dangerZone")}</span>
            <p className="text-xs text-muted" style={{ margin: 0 }}>
              {t("apps.dangerZoneDesc")}
            </p>
            <div>
              <button
                type="button"
                className="secondary-button"
                style={{ color: "var(--danger)", borderColor: "color-mix(in srgb, var(--danger) 40%, var(--border))" }}
                onClick={handleDeleteApp}
              >
                <Trash2 size={16} />
                {t("apps.deleteApp")}
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </Modal>
  );
}

function AppStatsModal({
  application,
  onClose,
}: {
  application: Application;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [selectedDays, setSelectedDays] = useState<number | "all">(30);
  const [selectedOsFamily, setSelectedOsFamily] = useState<string>("all");
  const [stats, setStats] = useState<AppTelemetryStats | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    setLoading(true);
    const query = selectedDays === "all" ? "" : `?days=${selectedDays}`;
    api<AppTelemetryStats>(`/api/v1/admin/applications/${application.id}/stats${query}`)
      .then((res) => {
        setStats(res);
        setLoading(false);
      })
      .catch(() => {
        setLoading(false);
      });
  }, [application.id, selectedDays]);

  const filteredOperatingSystems = (stats?.operatingSystems ?? []).filter((os) => {
    if (selectedOsFamily === "all") return true;
    if (selectedOsFamily === "Linux") {
      return (
        os.name.toLowerCase().includes("linux") ||
        os.name.toLowerCase().includes("nixos") ||
        os.name.toLowerCase().includes("fedora") ||
        os.name.toLowerCase().includes("mint") ||
        os.name.toLowerCase().includes("ubuntu") ||
        os.name.toLowerCase().includes("debian")
      );
    }
    if (selectedOsFamily === "Windows") {
      return os.name.toLowerCase().startsWith("windows");
    }
    if (selectedOsFamily === "macOS") {
      return os.name.toLowerCase().includes("mac") || os.name.toLowerCase().includes("darwin");
    }
    return true;
  });

  const filteredBuilds = (stats?.buildDistribution ?? []).filter((b) => {
    if (selectedOsFamily === "all") return true;
    if (selectedOsFamily === "Linux") {
      return (
        b.name.toLowerCase().includes("linux") ||
        b.name.toLowerCase().includes("nixos") ||
        b.name.toLowerCase().includes("fedora") ||
        b.name.toLowerCase().includes("mint")
      );
    }
    if (selectedOsFamily === "Windows") {
      return !b.name.toLowerCase().includes("fedora") && !b.name.toLowerCase().includes("nixos") && !b.name.toLowerCase().includes("mint");
    }
    return true;
  });

  return (
    <Modal
      onClose={onClose}
      title={
        <div style={{ display: "flex", alignItems: "center", gap: "10px" }}>
          <span>{application.name}</span>
          <span
            style={{
              fontSize: "0.72rem",
              background: "var(--signal-subtle)",
              color: "var(--signal)",
              padding: "2px 8px",
              borderRadius: "12px",
              fontWeight: 600,
            }}
          >
            {application.slug}
          </span>
        </div>
      }
      subtitle={t("apps.statsRange")}
      size="lg"
    >
      <div className="stats-modal-body">
        {/* Time Range Switcher Header */}
        <div className="flex justify-between items-center" style={{ marginBottom: "16px", flexWrap: "wrap", gap: "12px" }}>
          <div className="segmented-control" style={{ minWidth: 280, gridTemplateColumns: "repeat(4, 1fr)" }}>
            <button
              type="button"
              aria-pressed={selectedDays === 1}
              onClick={() => setSelectedDays(1)}
            >
              {t("apps.stats24h")}
            </button>
            <button
              type="button"
              aria-pressed={selectedDays === 7}
              onClick={() => setSelectedDays(7)}
            >
              {t("apps.stats7d")}
            </button>
            <button
              type="button"
              aria-pressed={selectedDays === 30}
              onClick={() => setSelectedDays(30)}
            >
              {t("apps.stats30d")}
            </button>
            <button
              type="button"
              aria-pressed={selectedDays === "all"}
              onClick={() => setSelectedDays("all")}
            >
              {t("apps.statsAll")}
            </button>
          </div>

          <div style={{ display: "flex", gap: "8px" }}>
            <a
              href={`/p/${application.slug}`}
              target="_blank"
              rel="noreferrer"
              className="secondary-button compact"
              style={{ gap: "6px" }}
            >
              <ExternalLink size={14} />
              {t("public.publicPreview")}
            </a>
          </div>
        </div>

        {loading || !stats ? (
          <div className="skeleton-grid" style={{ minHeight: "360px" }}>
            <i /><i /><i /><i />
          </div>
        ) : (
          <div style={{ display: "flex", flexDirection: "column", gap: "14px" }}>
            {/* Top Grid: Overview Dashboard (Left) + Version Distribution Donut (Right) */}
            <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(360px, 1fr))", gap: "14px" }}>
              {/* Top Left: 概览仪表板 (Overview Dashboard) */}
              <div className="distribution-card" style={{ display: "flex", flexDirection: "column", overflow: "hidden" }}>
                <div className="flex justify-between items-center" style={{ marginBottom: "12px" }}>
                  <h3 style={{ margin: 0 }}>{t("stats.overviewDash")}</h3>
                  <span className="status-chip"><i /> {t("stats.live")}</span>
                </div>

                {/* 3 Metric Headers: 总用户数, 日活 (DAU), 月活 (MAU) */}
                <div style={{ display: "grid", gridTemplateColumns: "repeat(3, 1fr)", gap: "12px", marginBottom: "16px" }}>
                  <div>
                    <span style={{ fontSize: "0.72rem", color: "var(--muted)" }}>{t("stats.totalUsers")}</span>
                    <div style={{ fontSize: "1.4rem", fontWeight: "800", fontFamily: "var(--font-mono)", color: "var(--text)" }}>
                      {(stats.overview.totalUsers ?? stats.overview.activeUsers).toLocaleString()}
                    </div>
                  </div>
                  <div>
                    <span style={{ fontSize: "0.72rem", color: "var(--muted)" }}>{t("stats.dau")}</span>
                    <div style={{ fontSize: "1.4rem", fontWeight: "800", fontFamily: "var(--font-mono)", color: "var(--signal)" }}>
                      {(stats.overview.dau ?? stats.overview.activeUsers).toLocaleString()}
                    </div>
                  </div>
                  <div>
                    <span style={{ fontSize: "0.72rem", color: "var(--muted)" }}>{t("stats.mau")}</span>
                    <div style={{ fontSize: "1.4rem", fontWeight: "800", fontFamily: "var(--font-mono)", color: "var(--amber)" }}>
                      {(stats.overview.mau ?? stats.overview.activeUsers).toLocaleString()}
                    </div>
                  </div>
                </div>

                {/* Smooth Bezier Spline Area Chart */}
                <div style={{ flex: 1, minHeight: "160px" }}>
                  <SplineAreaChart
                    data={stats.trend.map((pt) => ({
                      label: pt.day,
                      value: pt.events,
                      secondaryValue: pt.users,
                    }))}
                    height={160}
                    strokeColor="#f97316"
                    fillColor="#f97316"
                    valueLabel={t("public.launches")}
                    secondaryLabel={t("public.devices")}
                  />
                </div>
              </div>

              {/* Top Right: 版本分布饼图 (Version Donut Chart) */}
              <DonutChart
                items={stats.appVersions}
                title={t("stats.versionDonut")}
                badge={t("stats.appVersion")}
                height={200}
              />
            </div>

            <ActivityStatsPanel activity={stats.activity} />

            {/* Bottom Grid: 3 Visual Analytic Cards */}
            <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(280px, 1fr))", gap: "14px" }}>
              {/* Bottom Left: 系统 Build (System Build Distribution) */}
              <div className="distribution-card" style={{ display: "flex", flexDirection: "column", overflow: "hidden" }}>
                <div className="flex justify-between items-center" style={{ marginBottom: "10px" }}>
                  <h3 style={{ margin: 0 }}>{t("stats.systemBuilds")}</h3>
                  <span
                    style={{
                      fontSize: "0.68rem",
                      fontWeight: "600",
                      background: "color-mix(in srgb, var(--amber) 20%, transparent)",
                      color: "var(--amber)",
                      padding: "2px 6px",
                      borderRadius: "6px",
                      border: "1px solid color-mix(in srgb, var(--amber) 30%, transparent)",
                    }}
                  >
                    {t("stats.build")}
                  </span>
                </div>
                <div style={{ flex: 1 }}>
                  <BuildBarChart
                    items={stats.buildDistribution ?? []}
                    height="100%"
                  />
                </div>
              </div>

              {/* Bottom Middle: 趋势分析 (Trend Ingestion Analysis) */}
              <div className="distribution-card" style={{ display: "flex", flexDirection: "column", overflow: "hidden" }}>
                <div className="flex justify-between items-center" style={{ marginBottom: "10px" }}>
                  <h3 style={{ margin: 0 }}>{t("stats.trendAnalysis")}</h3>
                  <span
                    style={{
                      fontSize: "0.68rem",
                      fontWeight: "600",
                      background: "color-mix(in srgb, var(--signal) 20%, transparent)",
                      color: "var(--signal)",
                      padding: "2px 6px",
                      borderRadius: "6px",
                      border: "1px solid color-mix(in srgb, var(--signal) 30%, transparent)",
                    }}
                  >
                    {t("apps.tabGrowth")}
                  </span>
                </div>
                <div style={{ flex: 1 }}>
                  <SplineAreaChart
                    data={stats.trend.map((pt) => ({
                      label: pt.day,
                      value: pt.events,
                      secondaryValue: pt.users,
                    }))}
                    height={160}
                    strokeColor="#818cf8"
                    fillColor="#818cf8"
                    valueLabel={t("explorer.events")}
                    secondaryLabel={t("public.devices")}
                  />
                </div>
              </div>

              {/* Bottom Right: 各版本用户增长/活跃对比 (Multi-Version Comparative Curves) */}
              <div className="distribution-card" style={{ display: "flex", flexDirection: "column", overflow: "hidden" }}>
                <div className="flex justify-between items-center" style={{ marginBottom: "10px" }}>
                  <h3 style={{ margin: 0 }}>{t("stats.versionCurves")}</h3>
                  <span
                    style={{
                      fontSize: "0.68rem",
                      fontWeight: "600",
                      background: "color-mix(in srgb, #34d399 20%, transparent)",
                      color: "#34d399",
                      padding: "2px 6px",
                      borderRadius: "6px",
                      border: "1px solid color-mix(in srgb, #34d399 30%, transparent)",
                    }}
                  >
                    {t("stats.compare")}
                  </span>
                </div>
                <div style={{ flex: 1, minHeight: "180px" }}>
                  <MultiLineChart
                    series={stats.versionSeries ?? []}
                    height={150}
                  />
                </div>
              </div>
            </div>

            {/* Operating System Platforms with Interactive Filter Tabs & Detailed List */}
            <div className="distribution-card" style={{ overflow: "hidden" }}>
              <div className="flex justify-between items-center" style={{ marginBottom: "12px", flexWrap: "wrap", gap: "8px" }}>
                <div className="flex items-center gap-2">
                  <h3 style={{ margin: 0 }}>{t("apps.osFamilies")}</h3>
                  <span className="text-xs text-muted">
                    ({filteredOperatingSystems.length} {t("apps.osDetailed")})
                  </span>
                </div>

                {/* Interactive Platform Filter Tabs */}
                <div style={{ display: "flex", gap: "6px", flexWrap: "wrap" }}>
                  <button
                    type="button"
                    style={{
                      fontSize: "0.72rem",
                      fontWeight: selectedOsFamily === "all" ? "700" : "500",
                      padding: "4px 10px",
                      borderRadius: "7px",
                      background: selectedOsFamily === "all" ? "var(--signal)" : "var(--input-bg)",
                      color: selectedOsFamily === "all" ? "#ffffff" : "var(--text)",
                      border: `1px solid ${selectedOsFamily === "all" ? "var(--signal)" : "var(--border-soft)"}`,
                      cursor: "pointer",
                      transition: "all 140ms ease",
                    }}
                    onClick={() => setSelectedOsFamily("all")}
                  >
                    {t("apps.statsAll")} ({stats.operatingSystems.length})
                  </button>

                  {(stats.osFamilies ?? []).map((fam) => {
                    const isSelected = selectedOsFamily === fam.name;
                    const isLinux = fam.name === "Linux";
                    return (
                      <button
                        key={fam.name}
                        type="button"
                        style={{
                          fontSize: "0.72rem",
                          fontWeight: isSelected ? "700" : "500",
                          padding: "4px 10px",
                          borderRadius: "7px",
                          background: isSelected
                            ? isLinux ? "var(--signal)" : "var(--amber)"
                            : "var(--input-bg)",
                          color: isSelected ? "#ffffff" : isLinux ? "var(--signal)" : "var(--text)",
                          border: `1px solid ${isSelected ? (isLinux ? "var(--signal)" : "var(--amber)") : "var(--border-soft)"}`,
                          cursor: "pointer",
                          display: "inline-flex",
                          alignItems: "center",
                          gap: "5px",
                          transition: "all 140ms ease",
                        }}
                        onClick={() => setSelectedOsFamily(isSelected ? "all" : fam.name)}
                      >
                        <span className="inline-flex items-center gap-1.5"><PlatformIcon platform={fam.name} size={12} /><span>{fam.name}</span></span>
                        <span style={{ opacity: 0.85, fontFamily: "var(--font-mono)", fontSize: "0.68rem" }}>
                          {fam.count.toLocaleString()} ({fam.percentage}%)
                        </span>
                      </button>
                    );
                  })}
                </div>
              </div>

              {/* Filtered Detailed OS List */}
              <div style={{ maxHeight: "200px", overflowY: "auto", paddingRight: "4px" }}>
                {filteredOperatingSystems.length === 0 ? (
                  <p className="text-xs text-muted" style={{ margin: "8px 0" }}>{t("apps.noOS")}</p>
                ) : (
                  filteredOperatingSystems.map((os) => {
                    const isLinux = os.name.toLowerCase().includes("linux") || os.name.toLowerCase().includes("nixos") || os.name.toLowerCase().includes("fedora") || os.name.toLowerCase().includes("mint");
                    return (
                      <div key={os.name} className="dist-row">
                        <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }} title={os.name}>
                          <strong style={{ color: isLinux ? "var(--signal)" : "var(--text)" }}>
                            {os.name}
                          </strong>
                        </span>
                        <div className="dist-bar-track">
                          <div
                            className="dist-bar-progress"
                            style={{
                              width: `${os.percentage}%`,
                              background: isLinux ? "var(--signal)" : "var(--amber)",
                            }}
                          />
                        </div>
                        <span className="text-muted">{os.count.toLocaleString()}</span>
                        <span className={`font-mono ${isLinux ? "text-signal" : "text-amber"}`}>
                          {os.percentage}%
                        </span>
                      </div>
                    );
                  })
                )}
              </div>
            </div>
          </div>
        )}
      </div>
    </Modal>
  );
}