import { useEffect, useState } from "react";
import {
  CheckCircle2,
  Database,
  Download,
  Globe,
  HardDrive,
  RefreshCw,
  Save,
  Server,
  ShieldCheck,
  ShieldAlert,
  Upload,
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
  const { t } = useTranslation();
  const [settings, setSettings] = useState<SystemSettings | null>(null);
  const [selectedTz, setSelectedTz] = useState("Asia/Shanghai");
  const [selectedLocale, setSelectedLocale] = useState("zh-CN");
  const [savingTz, setSavingTz] = useState(false);

  // 2FA State
  const [currentUser, setCurrentUser] = useState<User | null>(null);
  const [twoFactorSetup, setTwoFactorSetup] = useState<TwoFactorSetupData | null>(null);
  const [twoFactorQr, setTwoFactorQr] = useState<string | null>(null);
  const [enableCode, setEnableCode] = useState("");
  const [enablePassword, setEnablePassword] = useState("");
  const [twoFactorLoading, setTwoFactorLoading] = useState(false);
  const [showDisableModal, setShowDisableModal] = useState(false);
  const [disableCode, setDisableCode] = useState("");
  const [disablePassword, setDisablePassword] = useState("");

  // Audit Logs State
  const [auditLogs, setAuditLogs] = useState<AuditLog[]>([]);
  const [auditPage, setAuditPage] = useState(1);
  const [auditHasMore, setAuditHasMore] = useState(false);
  const [auditLoading, setAuditLoading] = useState(false);

  // Backup State
  const [exporting, setExporting] = useState(false);
  const [restoring, setRestoring] = useState(false);
  const [restoreFile, setRestoreFile] = useState<unknown | null>(null);
  const [restoreFileName, setRestoreFileName] = useState("");
  const [error, setError] = useState("");
  const [successMsg, setSuccessMsg] = useState("");

  const refreshUser = () => {
    api<User>("/api/v1/auth/me")
      .then((u) => setCurrentUser(u))
      .catch(() => {});
  };

  const loadAuditLogs = (page: number) => {
    setAuditLoading(true);
    api<AuditLogPage>(`/api/v1/admin/audit?page=${page}&pageSize=15`)
      .then((res) => {
        setAuditLogs(res.items);
        setAuditPage(res.page);
        setAuditHasMore(res.hasMore);
      })
      .catch(() => {})
      .finally(() => setAuditLoading(false));
  };

  useEffect(() => {
    refreshUser();
    loadAuditLogs(1);
    api<SystemSettings>("/api/v1/admin/system/settings")
      .then((res) => {
        setSettings(res);
        if (res.timezone) setSelectedTz(res.timezone);
        if (res.locale) setSelectedLocale(res.locale);
      })
      .catch(() => {});
  }, []);

  const handleSaveSettings = async (e: React.FormEvent) => {
    e.preventDefault();
    setSavingTz(true);
    setError("");
    setSuccessMsg("");
    try {
      const res = await api<SystemSettings>("/api/v1/admin/system/settings", {
        method: "PATCH",
        body: JSON.stringify({
          timezone: selectedTz,
          locale: selectedLocale,
        }),
      });
      setSettings(res);
      setSuccessMsg("平台全局时区与系统偏好已成功更新并生效！全系统数据统计与日切点已基于该时区运行。");
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
      const setup = await api<TwoFactorSetupData>("/api/v1/auth/2fa/setup", {
        method: "POST",
      });
      setTwoFactorSetup(setup);
      setEnableCode("");
      setEnablePassword("");
      try {
        const qr = await QRCode.toDataURL(setup.otpauthUri, {
          margin: 2,
          width: 220,
          color: {
            dark: "#000000",
            light: "#ffffff",
          },
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

  const handleConfirmEnable2FA = async (e: React.FormEvent) => {
    e.preventDefault();
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

  const handleConfirmDisable2FA = async (e: React.FormEvent) => {
    e.preventDefault();
    setTwoFactorLoading(true);
    setError("");
    setSuccessMsg("");
    try {
      await api("/api/v1/auth/2fa/disable", {
        method: "POST",
        body: JSON.stringify({
          code: disableCode.trim(),
          password: disablePassword,
        }),
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

  const handleExportBackup = async () => {
    setExporting(true);
    setError("");
    try {
      const data = await api<unknown>("/api/v1/admin/system/backup");
      const blob = new Blob([JSON.stringify(data, null, 2)], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      const dateStr = new Date().toISOString().slice(0, 10);
      a.download = `sonde-full-backup-${dateStr}.json`;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      URL.revokeObjectURL(url);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t("common.error"));
    } finally {
      setExporting(false);
    }
  };

  const handleFileChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    setError("");
    setSuccessMsg("");
    const file = e.target.files?.[0];
    if (!file) return;
    setRestoreFileName(file.name);
    const reader = new FileReader();
    reader.onload = (evt) => {
      try {
        const text = evt.target?.result as string;
        const parsed = JSON.parse(text);
        if (parsed.backupType !== "sonde_full_backup") {
          throw new Error("File is not a valid Sonde full server backup archive.");
        }
        setRestoreFile(parsed);
      } catch (err) {
        setError(err instanceof Error ? err.message : "Invalid JSON file");
        setRestoreFile(null);
      }
    };
    reader.readAsText(file);
  };

  const handleRestoreBackup = async () => {
    if (!restoreFile) return;
    if (!window.confirm(t("settings.restoreConfirm"))) {
      return;
    }
    setRestoring(true);
    setError("");
    setSuccessMsg("");
    try {
      await api("/api/v1/admin/system/restore", {
        method: "POST",
        body: JSON.stringify(restoreFile),
      });
      setSuccessMsg(t("settings.restoreSuccess"));
      setRestoreFile(null);
      setRestoreFileName("");
      loadAuditLogs(1);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t("common.error"));
    } finally {
      setRestoring(false);
    }
  };

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const backupSummary = restoreFile as any;

  // Format current server time in selected timezone
  const formattedPlatformTime = (() => {
    try {
      return new Intl.DateTimeFormat("zh-CN", {
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
        <div
          style={{
            padding: "12px 16px",
            marginBottom: "16px",
            background: "color-mix(in srgb, var(--signal) 12%, transparent)",
            border: "1px solid var(--signal)",
            borderRadius: "8px",
            color: "var(--signal)",
            display: "flex",
            alignItems: "center",
            gap: "8px",
            fontSize: "0.85rem",
          }}
        >
          <CheckCircle2 size={16} />
          {successMsg}
        </div>
      ) : null}

      {/* Two-Factor Authentication (2FA) Card */}
      <div className="glass-panel p-6 mb-6">
        <div className="flex items-center justify-between gap-4 mb-4 pb-3 border-b border-[var(--border-soft)]">
          <div className="flex items-center gap-3">
            <div className="p-2.5 rounded-xl icon-squircle-green font-bold">
              <KeyRound size={18} />
            </div>
            <div>
              <h3 className="text-base font-bold text-[var(--text)] m-0">{t("settings.twoFactor")}</h3>
              <p className="text-xs text-[var(--muted)] m-0 mt-0.5">
                {t("settings.twoFactorDesc")}
              </p>
            </div>
          </div>

          <div className="flex items-center gap-2">
            {currentUser?.totpEnabled ? (
              <span className="inline-flex items-center gap-1.5 px-3 py-1 rounded-full text-xs font-semibold bg-[var(--signal)]/15 text-[var(--signal)] border border-[var(--signal)]/30">
                <ShieldCheck size={14} />
                {t("settings.twoFactorEnabled")}
              </span>
            ) : (
              <span className="inline-flex items-center gap-1.5 px-3 py-1 rounded-full text-xs font-semibold bg-[var(--amber)]/15 text-[var(--amber)] border border-[var(--amber)]/30">
                <ShieldAlert size={14} />
                {t("settings.twoFactorDisabled")}
              </span>
            )}
          </div>
        </div>

        {currentUser?.totpEnabled ? (
          <div className="flex items-center justify-between pt-2">
            <p className="text-xs text-[var(--muted)] m-0">
              您的账户已处于 TOTP 双因素认证保护中。每次登录均需输入 6 位动态验证码。
            </p>
            <button
              type="button"
              onClick={() => setShowDisableModal(true)}
              className="px-3.5 py-2 text-xs font-semibold rounded-xl border border-[var(--danger)]/40 text-[var(--danger)] hover:bg-[var(--danger)]/10 transition-colors cursor-pointer"
            >
              {t("settings.disableTwoFactor")}
            </button>
          </div>
        ) : twoFactorSetup ? (
          /* 2FA Setup View */
          <form onSubmit={handleConfirmEnable2FA} className="space-y-4 pt-2">
            <div className="flex flex-col md:flex-row items-center gap-6 p-4 rounded-xl border border-[var(--border-soft)] bg-[var(--input-bg)]">
              {twoFactorQr ? (
                <div className="flex flex-col items-center gap-2 p-3 bg-white rounded-2xl shadow-md border border-gray-200 flex-shrink-0">
                  <img
                    src={twoFactorQr}
                    alt="2FA TOTP QR Code"
                    className="w-44 h-44 rounded-lg block"
                  />
                  <span className="text-[11px] font-semibold text-gray-700">
                    使用身份验证器扫码
                  </span>
                </div>
              ) : null}

              <div className="flex-1 space-y-3 text-xs w-full">
                <div>
                  <h4 className="font-bold text-sm text-[var(--text)] m-0 mb-1">
                    第一步：扫描左侧二维码或手动导入密钥
                  </h4>
                  <p className="text-[var(--muted)] m-0 leading-relaxed">
                    打开 Google Authenticator、Microsoft Authenticator、1Password、Bitwarden 或 Apple 密码，扫描二维码即可自动添加。
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
                        setSuccessMsg("密钥已复制到剪贴板！");
                      }}
                      className="p-1.5 hover:text-white transition-colors cursor-pointer"
                      title="Copy secret"
                    >
                      <Copy size={15} />
                    </button>
                  </div>
                </div>

                <div>
                  <p className="text-[var(--muted)] m-0">
                    也可直接导入 URI：
                    <a
                      href={twoFactorSetup.otpauthUri}
                      className="text-[var(--signal)] underline ml-1 font-mono break-all"
                    >
                      {twoFactorSetup.otpauthUri}
                    </a>
                  </p>
                </div>
              </div>
            </div>

            <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
              <label className="field">
                <span>{t("settings.twoFactorConfirmCode")}</span>
                <input
                  type="text"
                  pattern="[0-9]*"
                  inputMode="numeric"
                  maxLength={6}
                  required
                  placeholder="123456"
                  value={enableCode}
                  onChange={(e) => setEnableCode(e.target.value.replace(/\D/g, "").slice(0, 6))}
                  className="input-select font-mono text-center tracking-widest text-base font-bold"
                />
              </label>

              <label className="field">
                <span>{t("settings.twoFactorConfirmPassword")}</span>
                <input
                  type="password"
                  required
                  placeholder="••••••••"
                  value={enablePassword}
                  onChange={(e) => setEnablePassword(e.target.value)}
                  className="input-select"
                />
              </label>
            </div>

            <div className="flex items-center gap-3 pt-2">
              <button
                type="submit"
                className="primary-button"
                style={{ width: "auto" }}
                disabled={twoFactorLoading || enableCode.length < 6 || !enablePassword}
              >
                {twoFactorLoading ? <RefreshCw size={14} className="animate-spin" /> : <ShieldCheck size={14} />}
                <span>确认绑定并启用</span>
              </button>
              <button
                type="button"
                onClick={() => setTwoFactorSetup(null)}
                className="secondary-button"
                style={{ width: "auto" }}
              >
                {t("common.cancel")}
              </button>
            </div>
          </form>
        ) : (
          <div className="flex items-center justify-between pt-2">
            <p className="text-xs text-[var(--muted)] m-0">
              启用 2FA 后，登录时需要同时提供密码与身份验证器动态码，防止密码泄露导致账户被盗。
            </p>
            <button
              type="button"
              onClick={handleStart2FASetup}
              disabled={twoFactorLoading}
              className="primary-button"
              style={{ width: "auto" }}
            >
              {twoFactorLoading ? <RefreshCw size={14} className="animate-spin" /> : <KeyRound size={14} />}
              <span>{t("settings.enableTwoFactor")}</span>
            </button>
          </div>
        )}

        {/* Disable 2FA Modal */}
        {showDisableModal ? (
          <div className="mt-4 pt-4 border-t border-[var(--border-soft)]">
            <form onSubmit={handleConfirmDisable2FA} className="space-y-3">
              <p className="text-xs font-semibold text-[var(--danger)] m-0">
                关闭 2FA 需要验证您的 6 位动态验证码及账户密码：
              </p>
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                <input
                  type="text"
                  pattern="[0-9]*"
                  inputMode="numeric"
                  maxLength={6}
                  required
                  placeholder="6 位动态验证码"
                  value={disableCode}
                  onChange={(e) => setDisableCode(e.target.value.replace(/\D/g, "").slice(0, 6))}
                  className="input-select font-mono text-center tracking-widest text-sm"
                />
                <input
                  type="password"
                  required
                  placeholder="当前账户密码"
                  value={disablePassword}
                  onChange={(e) => setDisablePassword(e.target.value)}
                  className="input-select text-sm"
                />
              </div>
              <div className="flex items-center gap-2">
                <button
                  type="submit"
                  disabled={twoFactorLoading || disableCode.length < 6 || !disablePassword}
                  className="px-4 py-2 bg-[var(--danger)] text-white text-xs font-bold rounded-xl hover:brightness-110 cursor-pointer disabled:opacity-50"
                >
                  {twoFactorLoading ? "验证中..." : "确认关闭 2FA"}
                </button>
                <button
                  type="button"
                  onClick={() => setShowDisableModal(false)}
                  className="secondary-button"
                  style={{ width: "auto" }}
                >
                  {t("common.cancel")}
                </button>
              </div>
            </form>
          </div>
        ) : null}
      </div>

      {/* Global Platform Timezone & Regional Settings */}
      <div className="glass-panel p-6 mb-6">
        <div className="flex items-center justify-between gap-4 mb-4 pb-3 border-b border-[var(--border-soft)]">
          <div className="flex items-center gap-3">
            <div className="p-2.5 rounded-xl icon-squircle-blue font-bold">
              <Globe size={18} />
            </div>
            <div>
              <h3 className="text-base font-bold text-[var(--text)] m-0">平台全局时区与偏好设置</h3>
              <p className="text-xs text-[var(--muted)] m-0 mt-0.5">
                配置 Sonde 遥测平台的统一基准时区，所有图表日切点、DAU/MAU 统计与数据聚合将严格基于该时区。
              </p>
            </div>
          </div>

          <div className="hidden sm:flex items-center gap-2 px-3 py-1.5 rounded-xl bg-[var(--input-bg)] border border-[var(--border-soft)] text-xs font-mono">
            <Clock size={13} className="text-[var(--signal)]" />
            <span className="text-[var(--muted)]">平台基准时间:</span>
            <strong className="text-[var(--text)]">{formattedPlatformTime}</strong>
          </div>
        </div>

        <form onSubmit={handleSaveSettings} className="space-y-4">
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            <label className="field">
              <span>全局基准时区 (IANA Timezone)</span>
              <select
                value={selectedTz}
                onChange={(e) => setSelectedTz(e.target.value)}
                className="input-select"
              >
                {POPULAR_TIMEZONES.map((tz) => (
                  <option key={tz.value} value={tz.value}>
                    {tz.label}
                  </option>
                ))}
              </select>
            </label>

            <label className="field">
              <span>系统默认语言 (Default Language)</span>
              <select
                value={selectedLocale}
                onChange={(e) => setSelectedLocale(e.target.value)}
                className="input-select"
              >
                <option value="zh-CN">简体中文 (Simplified Chinese)</option>
                <option value="en">English (US)</option>
              </select>
            </label>
          </div>

          <div className="flex justify-end pt-2">
            <button
              type="submit"
              className="primary-button"
              style={{ width: "auto" }}
              disabled={savingTz}
            >
              {savingTz ? <RefreshCw size={14} className="animate-spin" /> : <Save size={14} />}
              <span>{savingTz ? "保存中..." : "保存时区与偏好"}</span>
            </button>
          </div>
        </form>
      </div>

      {/* System Audit Logs Panel */}
      <div className="glass-panel p-6 mb-6">
        <div className="flex items-center justify-between gap-4 mb-4 pb-3 border-b border-[var(--border-soft)]">
          <div className="flex items-center gap-3">
            <div className="p-2.5 rounded-xl icon-squircle-purple font-bold">
              <FileText size={18} />
            </div>
            <div>
              <h3 className="text-base font-bold text-[var(--text)] m-0">{t("settings.auditLogs")}</h3>
              <p className="text-xs text-[var(--muted)] m-0 mt-0.5">
                {t("settings.auditDesc")}
              </p>
            </div>
          </div>

          <button
            type="button"
            onClick={() => loadAuditLogs(auditPage)}
            disabled={auditLoading}
            className="p-1.5 rounded-lg border border-[var(--border-soft)] text-[var(--muted)] hover:text-[var(--text)] transition-colors cursor-pointer"
            title="Refresh logs"
          >
            <RefreshCw size={14} className={auditLoading ? "animate-spin" : ""} />
          </button>
        </div>

        {auditLogs.length === 0 ? (
          <p className="text-xs text-[var(--muted)] text-center py-6">
            {t("settings.auditEmpty")}
          </p>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-xs text-left border-collapse">
              <thead>
                <tr className="border-b border-[var(--border-soft)] text-[var(--muted)]">
                  <th className="py-2.5 px-3 font-semibold">{t("settings.auditActor")}</th>
                  <th className="py-2.5 px-3 font-semibold">{t("settings.auditAction")}</th>
                  <th className="py-2.5 px-3 font-semibold">{t("settings.auditResource")}</th>
                  <th className="py-2.5 px-3 font-semibold">{t("settings.auditTime")}</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-[var(--border-soft)]">
                {auditLogs.map((log) => (
                  <tr key={log.id} className="hover:bg-[var(--panel-strong)]/50 transition-colors">
                    <td className="py-2.5 px-3 font-medium text-[var(--text)]">
                      {log.actorUsername || log.actorUserId || "System"}
                    </td>
                    <td className="py-2.5 px-3">
                      <span className="inline-block px-2 py-0.5 rounded bg-[var(--input-bg)] border border-[var(--border-soft)] font-mono text-[11px] text-[var(--signal)]">
                        {log.action}
                      </span>
                    </td>
                    <td className="py-2.5 px-3 text-[var(--muted)] font-mono text-[11px]">
                      {log.resourceType}{log.resourceId ? `:${log.resourceId.slice(0, 8)}` : ""}
                    </td>
                    <td className="py-2.5 px-3 text-[var(--muted)]">
                      {new Date(log.createdAt).toLocaleString()}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}

        {/* Audit Logs Pagination */}
        <div className="flex items-center justify-between pt-4 mt-2 border-t border-[var(--border-soft)]">
          <span className="text-xs text-[var(--muted)]">
            第 {auditPage} 页
          </span>
          <div className="flex items-center gap-2">
            <button
              type="button"
              disabled={auditPage <= 1 || auditLoading}
              onClick={() => loadAuditLogs(auditPage - 1)}
              className="p-1.5 rounded-lg border border-[var(--border-soft)] text-xs text-[var(--text)] disabled:opacity-40 cursor-pointer"
            >
              <ChevronLeft size={14} />
            </button>
            <button
              type="button"
              disabled={!auditHasMore || auditLoading}
              onClick={() => loadAuditLogs(auditPage + 1)}
              className="p-1.5 rounded-lg border border-[var(--border-soft)] text-xs text-[var(--text)] disabled:opacity-40 cursor-pointer"
            >
              <ChevronRight size={14} />
            </button>
          </div>
        </div>
      </div>

      <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(320px, 1fr))", gap: "16px", marginBottom: "24px" }}>
        <div
          style={{
            background: "var(--panel)",
            border: "1px solid var(--border-soft)",
            borderRadius: "10px",
            padding: "20px",
            display: "flex",
            flexDirection: "column",
            gap: "14px",
          }}
        >
          <div className="flex items-center gap-3">
            <Server size={20} className="text-signal" />
            <h2 style={{ margin: 0, fontSize: "1.05rem" }}>{t("settings.serverInfo")}</h2>
          </div>
          <div className="flex flex-col gap-2 text-xs">
            <div className="flex justify-between py-1 border-b border-border-soft">
              <span className="text-muted">{t("settings.version")}</span>
              <strong>v0.1.0-release</strong>
            </div>
            <div className="flex justify-between py-1 border-b border-border-soft">
              <span className="text-muted">{t("settings.runtimeMode")}</span>
              <code className="text-signal">Production (WAL)</code>
            </div>
            <div className="flex justify-between py-1 border-b border-border-soft">
              <span className="text-muted">{t("settings.security")}</span>
              <span className="text-signal flex items-center gap-1">
                <ShieldCheck size={13} /> Argon2id + RFC 6238 TOTP
              </span>
            </div>
          </div>
        </div>

        <div
          style={{
            background: "var(--panel)",
            border: "1px solid var(--border-soft)",
            borderRadius: "10px",
            padding: "20px",
            display: "flex",
            flexDirection: "column",
            gap: "14px",
          }}
        >
          <div className="flex items-center gap-3">
            <Database size={20} className="text-amber" />
            <h2 style={{ margin: 0, fontSize: "1.05rem" }}>{t("settings.storage")}</h2>
          </div>
          <div className="flex flex-col gap-2 text-xs">
            <div className="flex justify-between py-1 border-b border-border-soft">
              <span className="text-muted">{t("settings.databaseBackend")}</span>
              <strong>Multi-Dialect (SQLite / Postgres / MySQL)</strong>
            </div>
            <div className="flex justify-between py-1 border-b border-border-soft">
              <span className="text-muted">{t("settings.journalMode")}</span>
              <code>WAL (Write-Ahead Logging)</code>
            </div>
            <div className="flex justify-between py-1 border-b border-border-soft">
              <span className="text-muted">{t("settings.foreignKeys")}</span>
              <span className="text-signal font-semibold">{t("settings.enabled")}</span>
            </div>
          </div>
        </div>
      </div>

      <div
        style={{
          background: "var(--panel)",
          border: "1px solid var(--border)",
          borderRadius: "12px",
          padding: "24px",
          display: "flex",
          flexDirection: "column",
          gap: "20px",
        }}
      >
        <div>
          <div className="flex items-center gap-2 mb-1">
            <HardDrive size={22} className="text-signal" />
            <h2 style={{ margin: 0, fontSize: "1.2rem" }}>{t("settings.backupRestore")}</h2>
          </div>
          <p className="text-xs text-muted" style={{ margin: 0, lineHeight: 1.6 }}>
            {t("settings.backupDesc")}
          </p>
        </div>

        <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(300px, 1fr))", gap: "20px" }}>
          <div
            style={{
              background: "var(--panel-strong)",
              border: "1px solid var(--border-soft)",
              borderRadius: "8px",
              padding: "18px",
              display: "flex",
              flexDirection: "column",
              justifyContent: "space-between",
              gap: "16px",
            }}
          >
            <div>
              <strong style={{ display: "block", fontSize: "0.95rem", marginBottom: "6px" }}>
                {t("settings.exportBackup")}
              </strong>
              <p className="text-xs text-muted" style={{ margin: 0 }}>
                Download a complete JSON backup archive of all tables, users, applications, and telemetry logs.
              </p>
            </div>
            <div>
              <button
                type="button"
                className="primary-button"
                style={{ width: "auto" }}
                disabled={exporting}
                onClick={handleExportBackup}
              >
                {exporting ? <RefreshCw size={15} className="animate-spin" /> : <Download size={15} />}
                {exporting ? t("common.loading") : t("settings.exportBackup")}
              </button>
            </div>
          </div>

          <div
            style={{
              background: "var(--panel-strong)",
              border: "1px solid var(--border-soft)",
              borderRadius: "8px",
              padding: "18px",
              display: "flex",
              flexDirection: "column",
              gap: "14px",
            }}
          >
            <div>
              <strong style={{ display: "block", fontSize: "0.95rem", marginBottom: "6px" }}>
                {t("settings.restoreBackup")}
              </strong>
              <p className="text-xs text-muted" style={{ margin: 0 }}>
                {t("settings.restoreDesc")}
              </p>
            </div>

            <label className="field">
              <span>{t("settings.selectBackupFile")}</span>
              <input
                type="file"
                accept=".json"
                onChange={handleFileChange}
                style={{ padding: "8px" }}
              />
            </label>

            {backupSummary ? (
              <div
                style={{
                  background: "var(--input-bg)",
                  border: "1px solid var(--border-soft)",
                  borderRadius: "6px",
                  padding: "10px",
                  fontSize: "0.75rem",
                  display: "flex",
                  flexDirection: "column",
                  gap: "4px",
                }}
              >
                <div className="flex justify-between">
                  <span className="text-muted">Exported At:</span>
                  <span>{new Date(backupSummary.exportedAt).toLocaleString()}</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-muted">Users:</span>
                  <strong>{backupSummary.users?.length ?? 0}</strong>
                </div>
                <div className="flex justify-between">
                  <span className="text-muted">Applications:</span>
                  <strong>{backupSummary.applications?.length ?? 0}</strong>
                </div>
                <div className="flex justify-between">
                  <span className="text-muted">Events:</span>
                  <strong>{backupSummary.events?.length ?? 0}</strong>
                </div>
              </div>
            ) : null}

            <div>
              <button
                type="button"
                className="secondary-button"
                style={{
                  width: "auto",
                  color: "var(--amber)",
                  borderColor: "color-mix(in srgb, var(--amber) 40%, var(--border))",
                }}
                disabled={!restoreFile || restoring}
                onClick={handleRestoreBackup}
              >
                {restoring ? <RefreshCw size={15} className="animate-spin" /> : <Upload size={15} />}
                {restoring ? t("common.loading") : t("settings.restoreBackup")}
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
