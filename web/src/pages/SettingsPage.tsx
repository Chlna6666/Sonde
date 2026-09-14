import { useEffect, useState } from "react";
import {
  CheckCircle2,
  Database,
  Globe,
  RefreshCw,
  Save,
  Server,
  ShieldCheck,
  ShieldAlert,
  Clock,
  KeyRound,
  FileText,
  Copy,
  ChevronLeft,
  ChevronRight,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import QRCode from "qrcode";
import { api } from "../lib/api";
import type { User } from "../App";
import { Button, Card, Badge, Table, TableHeader, TableBody, TableRow, TableHead, TableCell, Input, EmptyState } from "../components/ui";

type SystemSettings = {
  timezone: string;
  locale: string;
  secureCookie: boolean;
  serverTime: number;
};

type TwoFactorSetupData = {
  secret: string;
  otpauthUri: string;
};

type AuditLog = {
  id: string;
  actorUserId?: string;
  actorUsername?: string;
  action: string;
  resourceType: string;
  resourceId?: string;
  metadata: unknown;
  createdAt: number;
};

type AuditLogPage = {
  items: AuditLog[];
  page: number;
  pageSize: number;
  hasMore: boolean;
};

const POPULAR_TIMEZONES = [
  { value: "Asia/Shanghai", label: "Asia/Shanghai (北京/上海/香港, UTC+8)" },
  { value: "UTC", label: "UTC (Coordinated Universal Time, UTC+0)" },
  { value: "Asia/Tokyo", label: "Asia/Tokyo (东京/首尔, UTC+9)" },
  { value: "Asia/Singapore", label: "Asia/Singapore (新加坡, UTC+8)" },
  { value: "Europe/London", label: "Europe/London (伦敦/都柏林, UTC+0/+1)" },
  { value: "Europe/Paris", label: "Europe/Paris (巴黎/柏林/罗马, UTC+1/+2)" },
  { value: "America/New_York", label: "America/New_York (纽约/东部时间, UTC-5/-4)" },
  { value: "America/Chicago", label: "America/Chicago (芝加哥/中部时间, UTC-6/-5)" },
  { value: "America/Los_Angeles", label: "America/Los_Angeles (洛杉矶/太平洋, UTC-8/-7)" },
];

export function SettingsPage() {
  const { t, i18n } = useTranslation();
  const [settings, setSettings] = useState<SystemSettings | null>(null);
  const [selectedTz, setSelectedTz] = useState("Asia/Shanghai");
  const [selectedLocale, setSelectedLocale] = useState("zh-CN");
  const [savingTz, setSavingTz] = useState(false);
  const [currentUser, setCurrentUser] = useState<User | null>(null);
  const [twoFactorSetup, setTwoFactorSetup] = useState<TwoFactorSetupData | null>(null);
  const [twoFactorQr, setTwoFactorQr] = useState<string | null>(null);
  const [enableCode, setEnableCode] = useState("");
  const [enablePassword, setEnablePassword] = useState("");
  const [twoFactorLoading, setTwoFactorLoading] = useState(false);
  const [showDisableModal, setShowDisableModal] = useState(false);
  const [disableCode, setDisableCode] = useState("");
  const [disablePassword, setDisablePassword] = useState("");
  const [auditLogs, setAuditLogs] = useState<AuditLog[]>([]);
  const [auditPage, setAuditPage] = useState(1);
  const [auditHasMore, setAuditHasMore] = useState(false);
  const [auditLoading, setAuditLoading] = useState(false);
  const [error, setError] = useState("");
  const [successMsg, setSuccessMsg] = useState("");

  const refreshUser = () => {
    api<User>("/api/v1/auth/me")
      .then((user) => setCurrentUser(user))
      .catch(() => {});
  };

  const loadAuditLogs = (page: number) => {
    setAuditLoading(true);
    api<AuditLogPage>(`/api/v1/admin/audit?page=${page}&pageSize=15`)
      .then((response) => {
        setAuditLogs(response.items);
        setAuditPage(response.page);
        setAuditHasMore(response.hasMore);
      })
      .catch(() => {})
      .finally(() => setAuditLoading(false));
  };

  useEffect(() => {
    refreshUser();
    loadAuditLogs(1);
    api<SystemSettings>("/api/v1/admin/system/settings")
      .then((response) => {
        setSettings(response);
        if (response.timezone) setSelectedTz(response.timezone);
        if (response.locale) setSelectedLocale(response.locale);
      })
      .catch(() => {});
  }, []);

  const handleSaveSettings = async (event: React.FormEvent) => {
    event.preventDefault();
    setSavingTz(true);
    setError("");
    setSuccessMsg("");
    try {
      const response = await api<SystemSettings>("/api/v1/admin/system/settings", {
        method: "PATCH",
        body: JSON.stringify({ timezone: selectedTz, locale: selectedLocale }),
      });
      setSettings(response);
      setSuccessMsg(t("settings.timezoneUpdated"));
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t("common.error"));
    } finally {
      setSavingTz(false);
    }
  };

  const handleStart2FASetup = async () => {
    setTwoFactorLoading(true);
    setError("");
    setSuccessMsg("");
    try {
      const setup = await api<TwoFactorSetupData>("/api/v1/auth/2fa/setup", { method: "POST" });
      setTwoFactorSetup(setup);
      setEnableCode("");
      setEnablePassword("");
      try {
        const qr = await QRCode.toDataURL(setup.otpauthUri, {
          margin: 2,
          width: 220,
          color: { dark: "#000000", light: "#ffffff" },
        });
        setTwoFactorQr(qr);
      } catch {
        setTwoFactorQr(null);
      }
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t("common.error"));
    } finally {
      setTwoFactorLoading(false);
    }
  };

  const handleConfirmEnable2FA = async (event: React.FormEvent) => {
    event.preventDefault();
    if (!twoFactorSetup) return;
    setTwoFactorLoading(true);
    setError("");
    setSuccessMsg("");
    try {
      await api("/api/v1/auth/2fa/enable", {
        method: "POST",
        body: JSON.stringify({
          secret: twoFactorSetup.secret,
          code: enableCode.trim(),
          password: enablePassword,
        }),
      });
      setSuccessMsg(t("settings.twoFactorEnableSuccess"));
      setTwoFactorSetup(null);
      refreshUser();
      loadAuditLogs(1);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t("common.error"));
    } finally {
      setTwoFactorLoading(false);
    }
  };

  const handleConfirmDisable2FA = async (event: React.FormEvent) => {
    event.preventDefault();
    setTwoFactorLoading(true);
    setError("");
    setSuccessMsg("");
    try {
      await api("/api/v1/auth/2fa/disable", {
        method: "POST",
        body: JSON.stringify({ code: disableCode.trim(), password: disablePassword }),
      });
      setSuccessMsg(t("settings.twoFactorDisableSuccess"));
      setShowDisableModal(false);
      setDisableCode("");
      setDisablePassword("");
      refreshUser();
      loadAuditLogs(1);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t("common.error"));
    } finally {
      setTwoFactorLoading(false);
    }
  };

  const formattedPlatformTime = (() => {
    try {
      const loc = i18n.language.startsWith("zh") ? "zh-CN" : "en-US";
      return new Intl.DateTimeFormat(loc, {
        timeZone: selectedTz,
        year: "numeric",
        month: "2-digit",
        day: "2-digit",
        hour: "2-digit",
        minute: "2-digit",
        second: "2-digit",
        hour12: false,
      }).format(new Date());
    } catch {
      return new Date().toLocaleString();
    }
  })();

  return (
    <div className="page enter-page">
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 mb-4">
        <div>
          <h2 className="text-xl font-bold tracking-tight text-[var(--text)] m-0">{t("settings.title")}</h2>
          <p className="text-xs text-[var(--muted)] mt-0.5">{t("settings.subtitle")}</p>
        </div>
      </div>

      {error ? <div className="form-error mb-4">{error}</div> : null}
      {successMsg ? (
        <div className="flex items-center gap-2 p-3 mb-4 rounded-[var(--radius-lg)] bg-[var(--signal-subtle)] border border-[var(--signal)]/30 text-[var(--signal)] text-xs font-medium">
          <CheckCircle2 size={16} />
          {successMsg}
        </div>
      ) : null}

      <Card className="p-5 mb-6">
        <div className="flex items-center justify-between gap-4 mb-4 pb-3 border-b border-[var(--border-soft)]">
          <div className="flex items-center gap-3">
            <div className="p-1.5 rounded-[var(--radius-sm)] icon-squircle-green font-bold">
              <KeyRound size={18} />
            </div>
            <div>
              <h3 className="text-sm sm:text-base font-bold text-[var(--text)] m-0">{t("settings.twoFactor")}</h3>
              <p className="text-xs text-[var(--muted)] m-0 mt-0.5">{t("settings.twoFactorDesc")}</p>
            </div>
          </div>
          <div className="flex items-center gap-2">
            {currentUser?.totpEnabled ? (
              <Badge variant="success" dot size="sm">
                <ShieldCheck size={13} />
                <span>{t("settings.twoFactorEnabled")}</span>
              </Badge>
            ) : (
              <Badge variant="warning" dot size="sm">
                <ShieldAlert size={13} />
                <span>{t("settings.twoFactorDisabled")}</span>
              </Badge>
            )}
          </div>
        </div>

        {currentUser?.totpEnabled ? (
          <div className="flex items-center justify-between pt-2">
            <p className="text-xs text-[var(--muted)] m-0">
              {t("settings.twoFactorProtected")}
            </p>
            <Button
              variant="danger-outline"
              size="sm"
              onClick={() => setShowDisableModal(true)}
            >
              {t("settings.disableTwoFactor")}
            </Button>
          </div>
        ) : twoFactorSetup ? (
          <form onSubmit={handleConfirmEnable2FA} className="space-y-4 pt-2">
            <div className="flex flex-col md:flex-row items-center gap-6 p-4 rounded-xl border border-[var(--border-soft)] bg-[var(--input-bg)]">
              {twoFactorQr ? (
                <div className="flex flex-col items-center gap-2 p-3 bg-white rounded-[var(--radius-md)] border border-gray-200 flex-shrink-0">
                  <img src={twoFactorQr} alt="2FA TOTP QR Code" className="w-44 h-44 rounded-lg block" />
                  <span className="text-[11px] font-semibold text-gray-700">{t("settings.twoFactorScanWithApp")}</span>
                </div>
              ) : null}

              <div className="flex-1 space-y-3 text-xs w-full">
                <div>
                  <h4 className="font-bold text-sm text-[var(--text)] m-0 mb-1">{t("settings.twoFactorStep1")}</h4>
                  <p className="text-[var(--muted)] m-0 leading-relaxed">
                    {t("settings.twoFactorStep1Desc")}
                  </p>
                </div>
                <div className="space-y-1.5">
                  <span className="text-[var(--muted)] font-medium">手动输入密钥 (Secret Key):</span>
                  <div className="flex items-center gap-2 font-mono bg-[var(--panel-strong)] p-2.5 rounded-lg border border-[var(--border)] text-[var(--signal)] font-bold text-sm">
                    <span className="flex-1 select-all tracking-wider">{twoFactorSetup.secret}</span>
                    <button
                      type="button"
                      onClick={() => {
                        navigator.clipboard.writeText(twoFactorSetup.secret);
                        setSuccessMsg(t("settings.twoFactorSecretCopied"));
                      }}
                      className="p-1.5 hover:text-white transition-colors cursor-pointer"
                      title="Copy secret"
                    >
                      <Copy size={15} />
                    </button>
                  </div>
                </div>
                <p className="text-[var(--muted)] m-0">
                  {t("settings.twoFactorUriPrompt")}
                  <a href={twoFactorSetup.otpauthUri} className="text-[var(--signal)] underline ml-1 font-mono break-all">
                    {twoFactorSetup.otpauthUri}
                  </a>
                </p>
              </div>
            </div>

            <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
              <label className="flex flex-col gap-1.5 text-xs text-[var(--muted)]">
                <span className="font-semibold text-[var(--text)]">{t("settings.twoFactorConfirmCode")}</span>
                <Input
                  type="text"
                  pattern="[0-9]*"
                  inputMode="numeric"
                  maxLength={6}
                  required
                  placeholder="123456"
                  value={enableCode}
                  onChange={(event) => setEnableCode(event.target.value.replace(/\D/g, "").slice(0, 6))}
                  className="font-mono text-center tracking-widest text-base font-bold"
                />
              </label>
              <label className="flex flex-col gap-1.5 text-xs text-[var(--muted)]">
                <span className="font-semibold text-[var(--text)]">{t("settings.twoFactorConfirmPassword")}</span>
                <Input
                  type="password"
                  required
                  placeholder="••••••••"
                  value={enablePassword}
                  onChange={(event) => setEnablePassword(event.target.value)}
                />
              </label>
            </div>

            <div className="flex items-center gap-2 pt-2">
              <Button
                type="submit"
                size="sm"
                loading={twoFactorLoading}
                disabled={enableCode.length < 6 || !enablePassword}
                icon={<ShieldCheck size={14} />}
              >
                {t("settings.twoFactorConfirmAndEnable")}
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={() => setTwoFactorSetup(null)}
              >
                {t("common.cancel")}
              </Button>
            </div>
          </form>
        ) : (
          <div className="flex items-center justify-between pt-2">
            <p className="text-xs text-[var(--muted)] m-0">
              {t("settings.twoFactorIntro")}
            </p>
            <Button
              type="button"
              size="sm"
              onClick={handleStart2FASetup}
              loading={twoFactorLoading}
              icon={<KeyRound size={14} />}
            >
              {t("settings.enableTwoFactor")}
            </Button>
          </div>
        )}

        {showDisableModal ? (
          <div className="mt-4 pt-4 border-t border-[var(--border-soft)]">
            <form onSubmit={handleConfirmDisable2FA} className="space-y-3">
              <p className="text-xs font-semibold text-[var(--danger)] m-0">{t("settings.twoFactorDisablePrompt")}</p>
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                <Input
                  type="text"
                  pattern="[0-9]*"
                  inputMode="numeric"
                  maxLength={6}
                  required
                  placeholder={t("settings.twoFactorCodePlaceholder")}
                  value={disableCode}
                  onChange={(event) => setDisableCode(event.target.value.replace(/\D/g, "").slice(0, 6))}
                  className="font-mono text-center tracking-widest text-sm"
                />
                <Input
                  type="password"
                  required
                  placeholder={t("settings.currentPasswordPlaceholder")}
                  value={disablePassword}
                  onChange={(event) => setDisablePassword(event.target.value)}
                />
              </div>
              <div className="flex items-center gap-2">
                <Button
                  type="submit"
                  variant="danger"
                  size="sm"
                  loading={twoFactorLoading}
                  disabled={disableCode.length < 6 || !disablePassword}
                >
                  {t("settings.twoFactorConfirmDisable")}
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  onClick={() => setShowDisableModal(false)}
                >
                  {t("common.cancel")}
                </Button>
              </div>
            </form>
          </div>
        ) : null}
      </Card>

      <Card className="p-5 mb-6">
        <div className="flex items-center justify-between gap-4 mb-4 pb-3 border-b border-[var(--border-soft)]">
          <div className="flex items-center gap-3">
            <div className="p-1.5 rounded-[var(--radius-sm)] icon-squircle-blue font-bold"><Globe size={18} /></div>
            <div>
              <h3 className="text-sm sm:text-base font-bold text-[var(--text)] m-0">{t("settings.timezoneTitle")}</h3>
              <p className="text-xs text-[var(--muted)] m-0 mt-0.5">{t("settings.timezoneDesc")}</p>
            </div>
          </div>
          <div className="hidden sm:flex items-center gap-2 px-3 py-1.5 rounded-[var(--radius-md)] bg-[var(--input-bg)] border border-[var(--border-soft)] text-xs font-mono">
            <Clock size={13} className="text-[var(--signal)]" />
            <span className="text-[var(--muted)]">{t("settings.platformTime")}:</span>
            <strong className="text-[var(--text)]">{formattedPlatformTime}</strong>
          </div>
        </div>

        <form onSubmit={handleSaveSettings} className="space-y-4">
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            <label className="flex flex-col gap-1.5 text-xs text-[var(--muted)]">
              <span className="font-semibold text-[var(--text)]">{t("settings.ianaTimezone")}</span>
              <select value={selectedTz} onChange={(event) => setSelectedTz(event.target.value)} className="h-9 px-3 rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--input-bg)] text-xs text-[var(--text)] focus:outline-none focus:ring-2 focus:ring-[var(--focus)]">
                {POPULAR_TIMEZONES.map((timezone) => (
                  <option key={timezone.value} value={timezone.value}>{timezone.label}</option>
                ))}
              </select>
            </label>
            <label className="flex flex-col gap-1.5 text-xs text-[var(--muted)]">
              <span className="font-semibold text-[var(--text)]">{t("settings.defaultLanguage")}</span>
              <select value={selectedLocale} onChange={(event) => setSelectedLocale(event.target.value)} className="h-9 px-3 rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--input-bg)] text-xs text-[var(--text)] focus:outline-none focus:ring-2 focus:ring-[var(--focus)]">
                <option value="zh-CN">简体中文 (Simplified Chinese)</option>
                <option value="en">English (US)</option>
              </select>
            </label>
          </div>
          <div className="flex justify-end pt-2">
            <Button
              type="submit"
              size="sm"
              loading={savingTz}
              icon={<Save size={14} />}
            >
              {savingTz ? t("settings.saving") : t("settings.saveTimezone")}
            </Button>
          </div>
        </form>
      </Card>

      <Card className="p-5 mb-6">
        <div className="flex items-center justify-between gap-4 mb-4 pb-3 border-b border-[var(--border-soft)]">
          <div className="flex items-center gap-3">
            <div className="p-1.5 rounded-[var(--radius-sm)] icon-squircle-purple font-bold"><FileText size={18} /></div>
            <div>
              <h3 className="text-sm sm:text-base font-bold text-[var(--text)] m-0">{t("settings.auditLogs")}</h3>
              <p className="text-xs text-[var(--muted)] m-0 mt-0.5">{t("settings.auditDesc")}</p>
            </div>
          </div>
          <Button
            variant="secondary"
            size="icon-sm"
            onClick={() => loadAuditLogs(auditPage)}
            disabled={auditLoading}
            icon={<RefreshCw size={13} className={auditLoading ? "animate-spin" : ""} />}
            title="Refresh logs"
          />
        </div>

        {auditLogs.length === 0 ? (
          <EmptyState
            icon={<FileText size={24} />}
            title={t("settings.auditEmpty")}
          />
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>{t("settings.auditActor")}</TableHead>
                <TableHead>{t("settings.auditAction")}</TableHead>
                <TableHead>{t("settings.auditResource")}</TableHead>
                <TableHead>{t("settings.auditTime")}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {auditLogs.map((log) => (
                <TableRow key={log.id}>
                  <TableCell className="font-medium">
                    {log.actorUsername || log.actorUserId || "System"}
                  </TableCell>
                  <TableCell>
                    <Badge variant="outline" size="sm" className="font-mono text-[11px] text-[var(--signal)]">
                      {log.action}
                    </Badge>
                  </TableCell>
                  <TableCell className="text-[var(--muted)] font-mono text-[11px]">
                    {log.resourceType}{log.resourceId ? `:${log.resourceId.slice(0, 8)}` : ""}
                  </TableCell>
                  <TableCell className="text-[var(--muted)]">
                    {new Date(log.createdAt).toLocaleString()}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}

        <div className="flex items-center justify-between pt-4 mt-2 border-t border-[var(--border-soft)]">
          <span className="text-xs text-[var(--muted)]">{t("settings.page", { page: auditPage })}</span>
          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="icon-sm"
              disabled={auditPage <= 1 || auditLoading}
              onClick={() => loadAuditLogs(auditPage - 1)}
              icon={<ChevronLeft size={14} />}
            />
            <Button
              variant="outline"
              size="icon-sm"
              disabled={!auditHasMore || auditLoading}
              onClick={() => loadAuditLogs(auditPage + 1)}
              icon={<ChevronRight size={14} />}
            />
          </div>
        </div>
      </Card>

      <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(320px, 1fr))", gap: "16px", marginBottom: "24px" }}>
        <div style={{ background: "var(--panel)", border: "1px solid var(--border-soft)", borderRadius: "10px", padding: "20px", display: "flex", flexDirection: "column", gap: "14px" }}>
          <div className="flex items-center gap-3">
            <Server size={20} className="text-signal" />
            <h2 style={{ margin: 0, fontSize: "1.05rem" }}>{t("settings.serverInfo")}</h2>
          </div>
          <div className="flex flex-col gap-2 text-xs">
            <div className="flex justify-between py-1 border-b border-border-soft"><span className="text-muted">{t("settings.version")}</span><strong>v0.1.0-release</strong></div>
            <div className="flex justify-between py-1 border-b border-border-soft"><span className="text-muted">{t("settings.runtimeMode")}</span><code className="text-signal">Production (WAL)</code></div>
            <div className="flex justify-between py-1 border-b border-border-soft">
              <span className="text-muted">{t("settings.security")}</span>
              <span className="text-signal flex items-center gap-1"><ShieldCheck size={13} /> Argon2id + RFC 6238 TOTP</span>
            </div>
          </div>
        </div>

        <div style={{ background: "var(--panel)", border: "1px solid var(--border-soft)", borderRadius: "10px", padding: "20px", display: "flex", flexDirection: "column", gap: "14px" }}>
          <div className="flex items-center gap-3">
            <Database size={20} className="text-amber" />
            <h2 style={{ margin: 0, fontSize: "1.05rem" }}>{t("settings.storage")}</h2>
          </div>
          <div className="flex flex-col gap-2 text-xs">
            <div className="flex justify-between py-1 border-b border-border-soft"><span className="text-muted">{t("settings.databaseBackend")}</span><strong>Multi-Dialect (SQLite / Postgres / MySQL)</strong></div>
            <div className="flex justify-between py-1 border-b border-border-soft"><span className="text-muted">{t("settings.journalMode")}</span><code>WAL (Write-Ahead Logging)</code></div>
            <div className="flex justify-between py-1 border-b border-border-soft"><span className="text-muted">{t("settings.foreignKeys")}</span><span className="text-signal font-semibold">{t("settings.enabled")}</span></div>
          </div>
        </div>
      </div>
    </div>
  );
}
