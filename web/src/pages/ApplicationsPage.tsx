import { FormEvent, useEffect, useState } from "react";
import {
  Activity,
  AppWindow,
  BarChart3,
  Check,
  Copy,
  Download,
  ExternalLink,
  Globe,
  Infinity as InfinityIcon,
  KeyRound,
  Plus,
  RefreshCw,
  Settings,
  ShieldCheck,
  Trash2,
  Upload,
  UserPlus,
  Users,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { Modal } from "../components/Modal";
import { SplineAreaChart } from "../components/SplineAreaChart";
import { DonutChart } from "../components/DonutChart";
import { MultiLineChart, type VersionSeriesData } from "../components/MultiLineChart";
import { BuildBarChart } from "../components/BuildBarChart";
import { ActivityStatsPanel, type ActivityStats } from "../components/ActivityStatsPanel";
import { PlatformIcon } from "../components/PlatformIcon";
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
          scopes: ["ingest"],
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
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = `${app.slug}-export.sonde.json`;
      document.body.appendChild(anchor);
      anchor.click();
      document.body.removeChild(anchor);
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
            <Upload size={14} />
            <span>{t("apps.importApp")}</span>
          </button>
          <button
            type="button"
            className="inline-flex items-center gap-1.5 px-3.5 py-1.5 rounded-xl bg-[var(--signal)] text-xs font-bold text-white shadow-sm hover:brightness-110 active:scale-95 transition-all cursor-pointer"
            onClick={() => setCreating(true)}
          >
            <Plus size={15} />
            <span>{t("apps.new")}</span>
          </button>
        </div>
      </div>

      {globalKeyReveal ? (
        <div className="key-reveal" role="status">
          <div>
            <KeyRound />
            <div>
              <strong>{t("apps.keyGenerated")}</strong>
              <code>{globalKeyReveal}</code>
              <small>{t("apps.keyGeneratedHint")}</small>
            </div>
          </div>
          <button type="button" onClick={() => navigator.clipboard.writeText(globalKeyReveal)} aria-label="Copy API key">
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
          <Field name="slug" label={t("apps.slug")} placeholder="flight-deck" pattern="[a-z0-9]+(?:-[a-z0-9]+)*" required />
          <div>
            <button className="primary-button compact">{t("common.create")}</button>
            <button type="button" className="secondary-button compact" onClick={() => setCreating(false)}>
              {t("common.cancel")}
            </button>
          </div>
        </form>
      ) : null}

      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
        {applications.map((application) => {
          const isPermanent = !application.retentionDays || application.retentionDays <= 0;
          return (
            <article key={application.id} className="glass-panel glass-panel-interactive p-6 flex flex-col justify-between group overflow-hidden">
              <div>
                <div className="flex items-start justify-between gap-2 mb-3">
                  <div className="flex items-center gap-3.5 min-w-0">
                    <div className="flex h-12 w-12 items-center justify-center rounded-2xl bg-[var(--signal-subtle)] text-[var(--signal)] border border-[var(--signal)]/25 shadow-xs flex-shrink-0 font-black text-lg">
                      {application.name.slice(0, 2).toUpperCase()}
                    </div>
                    <div className="min-w-0">
                      <div className="flex items-center gap-2 flex-wrap">
                        <h3 className="text-base font-bold text-[var(--text)] m-0 leading-tight truncate">{application.name}</h3>
                        {application.isPublic ? (
                          <span className="inline-flex items-center gap-1 rounded-full border border-[var(--signal)]/30 bg-[var(--signal-subtle)] px-2 py-0.5 text-[10px] font-bold text-[var(--signal)]">
                            <Globe size={10} /> Public
                          </span>
                        ) : (
                          <span className="inline-flex items-center gap-1 rounded-full border border-[var(--border)] bg-[var(--input-bg)] px-2 py-0.5 text-[10px] font-medium text-[var(--muted)]">Private</span>
                        )}
                      </div>
                      <div className="flex items-center gap-1.5 mt-1">
                        <code className="text-xs font-mono text-[var(--muted)]">{application.slug}</code>
                        <button type="button" className="text-[var(--muted)] hover:text-[var(--text)] p-0.5 cursor-pointer rounded" onClick={() => navigator.clipboard.writeText(application.slug)}>
                          <Copy size={11} />
                        </button>
                      </div>
                    </div>
                  </div>
                  <div className="flex items-center gap-1 flex-shrink-0">
                    {application.isPublic ? (
                      <a href={`/p/${application.slug}`} target="_blank" rel="noreferrer" className="p-1.5 rounded-xl text-[var(--muted)] hover:text-[var(--signal)] hover:bg-[var(--signal-subtle)]">
                        <ExternalLink size={15} />
                      </a>
                    ) : null}
                    <button type="button" className="p-1.5 rounded-xl text-[var(--muted)] hover:text-[var(--text)] hover:bg-[var(--input-bg)] cursor-pointer" onClick={() => handleExportApp(application)}>
                      <Download size={15} />
                    </button>
                  </div>
                </div>

                <div className="grid grid-cols-2 gap-3 my-5">
                  <div className="p-3.5 rounded-2xl bg-[var(--input-bg)] border border-[var(--border-soft)]">
                    <span className="text-[11px] font-medium text-[var(--muted)] block mb-1">{t("apps.retention")}</span>
                    {isPermanent ? (
                      <strong className="text-sm font-bold text-[var(--signal)] inline-flex items-center gap-1 mt-0.5"><InfinityIcon size={15} />永久保存</strong>
                    ) : (
                      <div className="flex items-baseline gap-1">
                        <strong className="text-lg font-extrabold font-mono text-[var(--text)]">{application.retentionDays}</strong>
                        <span className="text-xs text-[var(--muted)]">天</span>
                      </div>
                    )}
                  </div>
                  <div className="p-3.5 rounded-2xl bg-[var(--input-bg)] border border-[var(--border-soft)]">
                    <span className="text-[11px] font-medium text-[var(--muted)] block mb-1">{t("apps.status")}</span>
                    <div className="flex items-center gap-2 mt-1">
                      <span className="h-2 w-2 rounded-full bg-[var(--signal)] shadow-[0_0_8px_var(--signal)] animate-pulse" />
                      <strong className="text-xs font-bold text-[var(--signal)]">{t("apps.active")}</strong>
                    </div>
                  </div>
                </div>
              </div>

              <div className="grid grid-cols-2 gap-3 pt-4 border-t border-[var(--border-soft)]">
                <button type="button" className="h-10 px-3 rounded-xl bg-[var(--signal)] text-white text-xs font-bold inline-flex items-center justify-center gap-1.5 cursor-pointer" onClick={() => setStatsApp(application)}>
                  <BarChart3 size={14} />{t("apps.stats")}
                </button>
                <button type="button" className="h-10 px-3 rounded-xl border border-[var(--border)] bg-[var(--input-bg)] text-xs font-semibold text-[var(--text)] inline-flex items-center justify-center gap-1.5 cursor-pointer" onClick={() => setManagingApp(application)}>
                  <Settings size={14} />配置与 SDK
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
      {statsApp ? <AppStatsModal application={statsApp} onClose={() => setStatsApp(null)} /> : null}
      {importing ? <ImportAppModal onClose={() => setImporting(false)} onSuccess={() => { setImporting(false); load(); }} /> : null}
    </div>
  );
}

function Field({ name, label, placeholder, pattern, defaultValue, required }: {
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
      <input name={name} placeholder={placeholder} pattern={pattern} defaultValue={defaultValue} required={required} />
    </label>
  );
}

function ImportAppModal({ onClose, onSuccess }: { onClose: () => void; onSuccess: () => void }) {
  const { t } = useTranslation();
  const [fileContent, setFileContent] = useState<unknown | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);

  const handleFile = (event: React.ChangeEvent<HTMLInputElement>) => {
    setError("");
    const file = event.target.files?.[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = (loadEvent) => {
      try {
        const parsed = JSON.parse(String(loadEvent.target?.result ?? ""));
        if (!parsed.application?.name) throw new Error("Invalid Sonde application export file");
        setFileContent(parsed);
      } catch (cause) {
        setError(cause instanceof Error ? cause.message : "Failed to parse JSON file");
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
      await api("/api/v1/admin/applications/import", { method: "POST", body: JSON.stringify(fileContent) });
      onSuccess();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t("common.error"));
      setLoading(false);
    }
  };

  return (
    <Modal size="md" title={t("apps.importApp")} icon={<Upload size={20} />} onClose={onClose} footer={
      <div className="flex justify-end gap-3 w-full">
        <button type="button" className="secondary-button compact" onClick={onClose}>{t("common.cancel")}</button>
        <button type="button" className="primary-button compact" disabled={!fileContent || loading} onClick={handleImport}>{loading ? t("common.loading") : t("migration.execute")}</button>
      </div>
    }>
      <p className="text-xs text-muted" style={{ margin: 0 }}>{t("apps.importAppSubtitle")}</p>
      <label className="field">
        <span>{t("apps.importSelectFile")}</span>
        <input type="file" accept=".json,.sonde.json" onChange={handleFile} style={{ padding: "8px" }} />
      </label>
      {error ? <div className="form-error">{error}</div> : null}
    </Modal>
  );
}

function ManageAppModal({ application, onClose, onExport }: {
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
  const [revealedKey, setRevealedKey] = useState("");
  const [dependencyCopied, setDependencyCopied] = useState(false);
  const [exampleCopied, setExampleCopied] = useState(false);
  const [newKeyName, setNewKeyName] = useState("");
  const [newKeyEnvId, setNewKeyEnvId] = useState("");
  const [creatingKey, setCreatingKey] = useState(false);
  const [selectedUserId, setSelectedUserId] = useState("");
  const [selectedRole, setSelectedRole] = useState("Manager");
  const [isPublic, setIsPublic] = useState(application.isPublic ?? false);
  const [description, setDescription] = useState(application.description ?? "");
  const [githubUrl, setGithubUrl] = useState(application.githubUrl ?? "");
  const [websiteUrl, setWebsiteUrl] = useState(application.websiteUrl ?? "");
  const [customHeader, setCustomHeader] = useState(application.customHeader ?? "");

  const loadData = () => {
    api<Environment[]>(`/api/v1/admin/applications/${application.id}/environments`)
      .then((items) => {
        setEnvironments(items);
        if (items.length > 0) setNewKeyEnvId((current) => current || items[0].id);
      })
      .catch(() => {});
    api<ApiKey[]>(`/api/v1/admin/applications/${application.id}/keys`)
      .then((items) => {
        setApiKeys(items);
        setActiveKey(items.find((item) => item.isActive) ?? null);
      })
      .catch(() => {});
    api<AppMember[]>(`/api/v1/admin/applications/${application.id}/members`).then(setMembers).catch(() => {});
    api<UserSummary[]>("/api/v1/admin/users").then(setAllUsers).catch(() => {});
  };

  useEffect(loadData, [application.id]);

  const handleCreateKey = async (event: FormEvent) => {
    event.preventDefault();
    if (!newKeyName || !newKeyEnvId) return;
    try {
      const result = await api<{ id: string; key: string }>(`/api/v1/admin/applications/${application.id}/keys`, {
        method: "POST",
        body: JSON.stringify({ environmentId: newKeyEnvId, name: newKeyName, scopes: ["ingest"] }),
      });
      setRevealedKey(result.key);
      setNewKeyName("");
      setCreatingKey(false);
      loadData();
    } catch (cause) {
      alert(cause instanceof Error ? cause.message : t("common.error"));
    }
  };

  const handleRevokeKey = async (keyId: string) => {
    if (!window.confirm(t("apps.revokeConfirm"))) return;
    await api(`/api/v1/admin/applications/${application.id}/keys/${keyId}/revoke`, { method: "POST" });
    loadData();
  };

  const handleDeleteKey = async (keyId: string) => {
    if (!window.confirm(t("apps.deleteKeyConfirm"))) return;
    await api(`/api/v1/admin/applications/${application.id}/keys/${keyId}`, { method: "DELETE" });
    loadData();
  };

  const handleRegenerateKey = async (keyId: string) => {
    if (!window.confirm(t("apps.regenerateConfirm"))) return;
    const result = await api<{ key: string }>(`/api/v1/admin/applications/${application.id}/keys/${keyId}/regenerate`, { method: "POST" });
    setRevealedKey(result.key);
    loadData();
  };

  const handleAddMember = async (event: FormEvent) => {
    event.preventDefault();
    if (!selectedUserId) return;
    await api(`/api/v1/admin/applications/${application.id}/members`, {
      method: "POST",
      body: JSON.stringify({ userId: selectedUserId, role: selectedRole }),
    });
    setSelectedUserId("");
    loadData();
  };

  const handleRevokeMember = async (userId: string) => {
    await api(`/api/v1/admin/applications/${application.id}/members/${userId}`, { method: "DELETE" });
    loadData();
  };

  const handleUpdateSettings = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    try {
      await api(`/api/v1/admin/applications/${application.id}`, {
        method: "PATCH",
        body: JSON.stringify({
          name: String(form.get("name") || application.name),
          slug: String(form.get("slug") || application.slug),
          retentionDays: Number(form.get("retentionDays") || application.retentionDays),
          isPublic,
          description: description || null,
          githubUrl: githubUrl || null,
          websiteUrl: websiteUrl || null,
          customHeader: customHeader || null,
        }),
      });
      onClose();
    } catch (cause) {
      alert(cause instanceof Error ? cause.message : t("common.error"));
    }
  };

  const handleDeleteApp = async () => {
    if (!window.confirm(t("apps.deleteConfirm"))) return;
    await api(`/api/v1/admin/applications/${application.id}`, { method: "DELETE" });
    onClose();
  };

  const dependency = `sonde-sdk = { git = "https://github.com/Chlna6666/Sonde", package = "sonde-sdk" }`;
  const keyPlaceholder = revealedKey || activeKey?.keyPrefix ? "SONDE_BOOTSTRAP_KEY" : "SONDE_BOOTSTRAP_KEY";
  const example = `use sonde_sdk::{Event, SondeClient};\n\n#[tokio::main]\nasync fn main() -> sonde_sdk::Result<()> {\n    let sonde = SondeClient::builder(\n        "${window.location.origin}",\n        std::env::var("${keyPlaceholder}").expect("missing Sonde key"),\n        load_or_create_installation_id(),\n    )\n    .app_version(env!("CARGO_PKG_VERSION"))\n    .system_language("zh-CN")\n    .connect()\n    .await?;\n\n    sonde.event(\n        Event::new("app_startup").attribute("channel", "stable")\n    ).await?;\n\n    Ok(())\n}`;

  return (
    <Modal
      size="lg"
      title={application.name}
      subtitle={<code>{application.slug}</code>}
      icon={<AppWindow size={20} />}
      onClose={onClose}
      actions={<button type="button" className="secondary-button compact" onClick={onExport}><Download size={14} />{t("apps.exportApp")}</button>}
      tabs={
        <>
          {(["integration", "keys", "members", "public", "settings"] as const).map((tab) => (
            <button key={tab} type="button" className={`sonde-modal-tab ${activeTab === tab ? "active" : ""}`} onClick={() => setActiveTab(tab)}>
              {tab === "integration" ? <ShieldCheck size={15} /> : tab === "keys" ? <KeyRound size={15} /> : tab === "members" ? <Users size={15} /> : tab === "public" ? <Globe size={15} /> : <Settings size={15} />}
              {tab === "integration" ? "Rust SDK" : tab === "keys" ? `${t("apps.tabKeys")} (${apiKeys.length})` : tab === "members" ? `${t("apps.tabMembers")} (${members.length})` : tab === "public" ? t("apps.tabPublic") : t("apps.tabSettings")}
            </button>
          ))}
        </>
      }
    >
      {activeTab === "integration" ? (
        <div className="space-y-4">
          <div className="p-4 rounded-2xl bg-[var(--signal-subtle)] border border-[var(--signal)]/25">
            <div className="flex items-start gap-3">
              <ShieldCheck size={20} className="text-[var(--signal)] mt-0.5" />
              <div>
                <strong className="text-sm text-[var(--text)]">官方 Rust SDK · Git 分发</strong>
                <p className="text-xs text-[var(--muted)] mt-1 mb-0 leading-relaxed">
                  SDK 位于 Sonde 仓库的 <code>sdk/rust</code>，设置了 <code>publish = false</code>，不会发布到 crates.io。Token 刷新、HMAC、nonce、heartbeat 和平台字段所有权全部由 SDK 内部处理。
                </p>
              </div>
            </div>
          </div>

          <SdkCodeBlock
            title="Cargo.toml"
            value={dependency}
            copied={dependencyCopied}
            onCopy={() => {
              navigator.clipboard.writeText(dependency);
              setDependencyCopied(true);
              window.setTimeout(() => setDependencyCopied(false), 1600);
            }}
          />

          <div className="grid grid-cols-1 md:grid-cols-3 gap-3 text-xs">
            <SdkFact title="设备身份" text="应用只持久化一个高熵 installation/device ID；业务 telemetry 不发送 anonymousId。" />
            <SdkFact title="时间与 Session" text="客户端不上传 timestamp/sessionId。Sonde 使用服务端接收时间、heartbeat 和活动间隔推导。" />
            <SdkFact title="安全链路" text="Bootstrap Key 只换短期 sndt_ Token；实际上报自动签名 sonde-hmac-sha256-v2。" />
          </div>

          <SdkCodeBlock
            title="Minimal Rust integration"
            value={example}
            copied={exampleCopied}
            onCopy={() => {
              navigator.clipboard.writeText(example);
              setExampleCopied(true);
              window.setTimeout(() => setExampleCopied(false), 1600);
            }}
          />

          <div className="p-3.5 rounded-2xl bg-[var(--panel-strong)] border border-[var(--border-soft)] text-xs text-[var(--muted)] leading-relaxed">
            生产环境建议在依赖里增加 <code>rev = "&lt;commit sha&gt;"</code> 固定 SDK 版本。新建 Ingest Key 统一使用 <code>ingest</code> scope，它覆盖 heartbeat、events、metrics、logs 和 errors。
          </div>
        </div>
      ) : null}

      {activeTab === "keys" ? (
        <div className="space-y-4">
          {revealedKey ? (
            <div className="key-reveal" role="status">
              <div><KeyRound /><div><strong>{t("apps.newKeyCreated")}</strong><code>{revealedKey}</code><small>{t("apps.newKeyCreatedHint")}</small></div></div>
              <button type="button" onClick={() => navigator.clipboard.writeText(revealedKey)}><Copy size={16} /></button>
            </div>
          ) : null}
          <div className="keys-header">
            <span className="text-xs font-semibold text-muted uppercase">Ingest Keys</span>
            {!creatingKey ? <button type="button" className="primary-button compact" onClick={() => setCreatingKey(true)}><Plus size={15} />{t("apps.newKey")}</button> : null}
          </div>
          {creatingKey ? (
            <form className="inline-create" onSubmit={handleCreateKey}>
              <label className="field"><span>{t("apps.keyName")}</span><input value={newKeyName} onChange={(event) => setNewKeyName(event.target.value)} required /></label>
              <label className="field"><span>{t("apps.environment")}</span><select value={newKeyEnvId} onChange={(event) => setNewKeyEnvId(event.target.value)} required>{environments.map((environment) => <option key={environment.id} value={environment.id}>{environment.name}</option>)}</select></label>
              <div className="p-3 rounded-xl bg-[var(--input-bg)] border border-[var(--border-soft)] text-xs text-[var(--muted)]">固定权限：<code>ingest</code>（heartbeat + events + metrics + logs + errors）</div>
              <div className="flex gap-2"><button className="primary-button compact">{t("common.create")}</button><button type="button" className="secondary-button compact" onClick={() => setCreatingKey(false)}>{t("common.cancel")}</button></div>
            </form>
          ) : null}
          <div style={{ overflowX: "auto" }}>
            <table className="keys-table">
              <thead><tr><th>{t("apps.keyName")}</th><th>{t("apps.environment")}</th><th>Scope</th><th>{t("apps.status")}</th><th>{t("apps.createdAt")}</th><th /></tr></thead>
              <tbody>
                {apiKeys.map((key) => (
                  <tr key={key.id}>
                    <td><strong>{key.name}</strong></td>
                    <td><code>{key.environmentName}</code></td>
                    <td><code>{key.scopes.join(", ")}</code></td>
                    <td>{key.isActive ? <span className="status-ok"><i />{t("apps.active")}</span> : <span style={{ color: "var(--danger)" }}>{t("apps.revoked")}</span>}</td>
                    <td>{new Date(key.createdAt).toLocaleDateString()}</td>
                    <td><div className="key-actions">
                      {key.isActive ? <><button type="button" onClick={() => handleRegenerateKey(key.id)}><RefreshCw size={13} /></button><button type="button" className="danger" onClick={() => handleRevokeKey(key.id)}><Trash2 size={13} /></button></> : <button type="button" className="danger" onClick={() => handleDeleteKey(key.id)}><Trash2 size={13} /></button>}
                    </div></td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      ) : null}

      {activeTab === "members" ? (
        <div className="flex flex-col gap-6">
          <form className="inline-create" onSubmit={handleAddMember}>
            <label className="field"><span>{t("access.username")}</span><select value={selectedUserId} onChange={(event) => setSelectedUserId(event.target.value)} required><option value="">{t("apps.selectMember")}</option>{allUsers.map((user) => <option key={user.id} value={user.id}>{user.username} ({user.email})</option>)}</select></label>
            <label className="field"><span>{t("apps.memberRole")}</span><select value={selectedRole} onChange={(event) => setSelectedRole(event.target.value)}><option value="Manager">{t("apps.manager")}</option><option value="Viewer">{t("apps.viewer")}</option></select></label>
            <div><button className="primary-button compact"><UserPlus size={15} />{t("apps.grantMember")}</button></div>
          </form>
          <table className="keys-table"><thead><tr><th>{t("access.username")}</th><th>{t("access.email")}</th><th>{t("apps.memberRole")}</th><th /></tr></thead><tbody>
            {members.map((member) => <tr key={member.userId}><td><strong>{member.username}</strong></td><td>{member.email}</td><td>{member.role}</td><td><button type="button" className="secondary-button compact" style={{ color: "var(--danger)" }} onClick={() => handleRevokeMember(member.userId)}><Trash2 size={13} /></button></td></tr>)}
          </tbody></table>
        </div>
      ) : null}

      {activeTab === "public" ? (
        <form className="flex flex-col gap-4" onSubmit={handleUpdateSettings}>
          <label className="flex items-center justify-between p-4 rounded-xl bg-[var(--panel-strong)] border border-[var(--border)]"><div><strong>{t("apps.enablePublic")}</strong><p className="text-xs text-muted m-0 mt-1">Expose the public telemetry showcase page.</p></div><input type="checkbox" checked={isPublic} onChange={(event) => setIsPublic(event.target.checked)} /></label>
          <label className="field"><span>{t("apps.description")}</span><textarea value={description} onChange={(event) => setDescription(event.target.value)} rows={3} /></label>
          <label className="field"><span>{t("apps.githubUrl")}</span><input value={githubUrl} onChange={(event) => setGithubUrl(event.target.value)} /></label>
          <label className="field"><span>{t("apps.websiteUrl")}</span><input value={websiteUrl} onChange={(event) => setWebsiteUrl(event.target.value)} /></label>
          <label className="field"><span>{t("apps.customTagline")}</span><input value={customHeader} onChange={(event) => setCustomHeader(event.target.value)} /></label>
          <button className="primary-button">{t("apps.saveSettings")}</button>
        </form>
      ) : null}

      {activeTab === "settings" ? (
        <div className="flex flex-col gap-6">
          <form className="flex flex-col gap-4" onSubmit={handleUpdateSettings}>
            <Field name="name" label="Application Name" defaultValue={application.name} />
            <Field name="slug" label="URL Slug" pattern="[a-z0-9]+(?:-[a-z0-9]+)*" defaultValue={application.slug} />
            <label className="field"><span>{t("apps.retention")}</span><select name="retentionDays" defaultValue={application.retentionDays}><option value={30}>30 days</option><option value={90}>90 days</option><option value={180}>180 days</option><option value={365}>365 days</option><option value={730}>730 days</option><option value={3650}>3650 days</option></select></label>
            <button className="primary-button">{t("apps.saveSettings")}</button>
          </form>
          <hr style={{ borderColor: "var(--border-soft)" }} />
          <div><button type="button" className="secondary-button" style={{ color: "var(--danger)" }} onClick={handleDeleteApp}><Trash2 size={16} />{t("apps.deleteApp")}</button></div>
        </div>
      ) : null}
    </Modal>
  );
}

function SdkCodeBlock({ title, value, copied, onCopy }: { title: string; value: string; copied: boolean; onCopy: () => void }) {
  return (
    <div className="relative rounded-2xl bg-[#0d1117] border border-[var(--border)] overflow-hidden shadow-xl">
      <div className="flex items-center justify-between px-4 py-2 bg-[#161b22] border-b border-[#30363d] text-xs">
        <span className="font-mono text-gray-400 font-semibold">{title}</span>
        <button type="button" onClick={onCopy} className="inline-flex items-center gap-1.5 px-3 py-1 rounded-lg bg-[var(--signal)] text-white font-bold">{copied ? <Check size={13} /> : <Copy size={13} />}{copied ? "已复制" : "复制"}</button>
      </div>
      <pre className="p-4 text-xs font-mono text-gray-200 overflow-x-auto max-h-[420px] leading-relaxed select-text"><code>{value}</code></pre>
    </div>
  );
}

function SdkFact({ title, text }: { title: string; text: string }) {
  return <div className="p-3.5 rounded-2xl bg-[var(--panel-strong)] border border-[var(--border-soft)]"><strong className="text-[var(--text)]">{title}</strong><p className="m-0 mt-1 text-[11px] leading-relaxed text-[var(--muted)]">{text}</p></div>;
}

function AppStatsModal({ application, onClose }: { application: Application; onClose: () => void }) {
  const { t } = useTranslation();
  const [selectedDays, setSelectedDays] = useState<number | "all">(30);
  const [selectedOsFamily, setSelectedOsFamily] = useState("all");
  const [stats, setStats] = useState<AppTelemetryStats | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    setLoading(true);
    const query = selectedDays === "all" ? "" : `?days=${selectedDays}`;
    api<AppTelemetryStats>(`/api/v1/admin/applications/${application.id}/stats${query}`)
      .then((result) => { setStats(result); setLoading(false); })
      .catch(() => setLoading(false));
  }, [application.id, selectedDays]);

  const filteredOperatingSystems = (stats?.operatingSystems ?? []).filter((os) => {
    if (selectedOsFamily === "all") return true;
    const name = os.name.toLowerCase();
    if (selectedOsFamily === "Linux") return ["linux", "nixos", "fedora", "mint", "ubuntu", "debian"].some((part) => name.includes(part));
    if (selectedOsFamily === "Windows") return name.startsWith("windows");
    if (selectedOsFamily === "macOS") return name.includes("mac") || name.includes("darwin");
    return true;
  });

  return (
    <Modal onClose={onClose} title={<div className="flex items-center gap-2"><span>{application.name}</span><code className="text-xs text-[var(--signal)]">{application.slug}</code></div>} subtitle={t("apps.statsRange")} size="lg">
      <div className="stats-modal-body">
        <div className="flex justify-between items-center mb-4 flex-wrap gap-3">
          <div className="segmented-control" style={{ minWidth: 280, gridTemplateColumns: "repeat(4, 1fr)" }}>
            <button type="button" aria-pressed={selectedDays === 1} onClick={() => setSelectedDays(1)}>{t("apps.stats24h")}</button>
            <button type="button" aria-pressed={selectedDays === 7} onClick={() => setSelectedDays(7)}>{t("apps.stats7d")}</button>
            <button type="button" aria-pressed={selectedDays === 30} onClick={() => setSelectedDays(30)}>{t("apps.stats30d")}</button>
            <button type="button" aria-pressed={selectedDays === "all"} onClick={() => setSelectedDays("all")}>{t("apps.statsAll")}</button>
          </div>
          <a href={`/p/${application.slug}`} target="_blank" rel="noreferrer" className="secondary-button compact"><ExternalLink size={14} />{t("public.publicPreview")}</a>
        </div>

        {loading || !stats ? (
          <div className="skeleton-grid" style={{ minHeight: 360 }}><i /><i /><i /><i /></div>
        ) : (
          <div className="flex flex-col gap-4">
            <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
              <div className="distribution-card">
                <div className="flex justify-between items-center mb-3"><h3 className="m-0">{t("stats.overviewDash")}</h3><span className="status-chip"><i />{t("stats.live")}</span></div>
                <div className="grid grid-cols-3 gap-3 mb-4">
                  <StatValue label={t("stats.totalUsers")} value={stats.overview.totalUsers ?? stats.overview.activeUsers} />
                  <StatValue label={t("stats.dau")} value={stats.overview.dau ?? stats.overview.activeUsers} signal />
                  <StatValue label={t("stats.mau")} value={stats.overview.mau ?? stats.overview.activeUsers} />
                </div>
                <SplineAreaChart data={stats.trend.map((point) => ({ label: point.day, value: point.events, secondaryValue: point.users }))} height={170} strokeColor="#f97316" fillColor="#f97316" valueLabel={t("explorer.events")} secondaryLabel={t("public.devices")} />
              </div>
              <DonutChart items={stats.appVersions} title={t("stats.versionDonut")} badge={t("stats.appVersion")} height={220} />
            </div>

            <ActivityStatsPanel activity={stats.activity} />

            <div className="grid grid-cols-1 lg:grid-cols-3 gap-4">
              <div className="distribution-card"><h3>{t("stats.systemBuilds")}</h3><BuildBarChart items={stats.buildDistribution ?? []} height={180} /></div>
              <div className="distribution-card"><h3>{t("stats.trendAnalysis")}</h3><SplineAreaChart data={stats.trend.map((point) => ({ label: point.day, value: point.events, secondaryValue: point.users }))} height={160} strokeColor="#818cf8" fillColor="#818cf8" valueLabel={t("explorer.events")} secondaryLabel={t("public.devices")} /></div>
              <div className="distribution-card"><h3>{t("stats.versionCurves")}</h3><MultiLineChart series={stats.versionSeries ?? []} height={160} /></div>
            </div>

            <div className="distribution-card">
              <div className="flex justify-between items-center gap-3 flex-wrap mb-3">
                <h3 className="m-0">{t("apps.osFamilies")}</h3>
                <div className="flex gap-2 flex-wrap">
                  <button type="button" className="secondary-button compact" onClick={() => setSelectedOsFamily("all")}>{t("apps.statsAll")}</button>
                  {(stats.osFamilies ?? []).map((family) => <button key={family.name} type="button" className="secondary-button compact" onClick={() => setSelectedOsFamily(family.name)}><PlatformIcon platform={family.name} size={12} />{family.name} ({family.count})</button>)}
                </div>
              </div>
              <div style={{ maxHeight: 220, overflowY: "auto" }}>
                {filteredOperatingSystems.map((os) => <div key={os.name} className="dist-row"><span><strong>{os.name}</strong></span><div className="dist-bar-track"><div className="dist-bar-progress" style={{ width: `${os.percentage}%` }} /></div><span className="text-muted">{os.count.toLocaleString()}</span><span className="font-mono text-signal">{os.percentage}%</span></div>)}
              </div>
            </div>
          </div>
        )}
      </div>
    </Modal>
  );
}

function StatValue({ label, value, signal = false }: { label: string; value: number; signal?: boolean }) {
  return <div><span className="text-[0.72rem] text-[var(--muted)]">{label}</span><div className={`text-xl font-extrabold font-mono ${signal ? "text-[var(--signal)]" : "text-[var(--text)]"}`}>{value.toLocaleString()}</div></div>;
}
