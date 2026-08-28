import { useState } from "react";
import {
  ArchiveRestore,
  CheckCircle2,
  Download,
  FileArchive,
  HardDrive,
  RefreshCw,
  ShieldAlert,
  Upload,
} from "lucide-react";
import { api } from "../lib/api";

type BackupManifest = {
  formatVersion: string;
  backupType: string;
  exportedAt: number;
  serverVersion: string;
  containsSecrets: boolean;
  totpSecretsIncluded: boolean;
  ephemeralAuthStateIncluded: boolean;
  generatedRollupsIncluded: boolean;
};

type RestoreResponse = {
  ok: boolean;
  restoredRecords: number;
  formatVersion: string;
};

const BACKUP_TYPE = "sonde_full_backup_ndjson";
const FORMAT_VERSION = "2.0";
const MANIFEST_READ_BYTES = 64 * 1024;

export function BackupPage() {
  const [restoreFile, setRestoreFile] = useState<File | null>(null);
  const [manifest, setManifest] = useState<BackupManifest | null>(null);
  const [restoring, setRestoring] = useState(false);
  const [error, setError] = useState("");
  const [success, setSuccess] = useState("");

  const handleExport = () => {
    setError("");
    setSuccess("");
    const link = document.createElement("a");
    link.href = "/api/v1/admin/system/backup/v2";
    link.rel = "noopener";
    document.body.appendChild(link);
    link.click();
    document.body.removeChild(link);
  };

  const handleFileChange = async (event: React.ChangeEvent<HTMLInputElement>) => {
    setError("");
    setSuccess("");
    setRestoreFile(null);
    setManifest(null);

    const file = event.target.files?.[0];
    if (!file) return;

    try {
      if (file.size === 0) throw new Error("备份文件为空。");
      const prefix = await file.slice(0, Math.min(file.size, MANIFEST_READ_BYTES)).text();
      const newline = prefix.indexOf("\n");
      if (newline < 0) throw new Error("未找到 v2 NDJSON manifest 行。");
      const firstLine = prefix.slice(0, newline).replace(/\r$/, "");
      const record = JSON.parse(firstLine) as { type?: string; data?: BackupManifest };
      if (record.type !== "manifest" || !record.data) {
        throw new Error("文件不是 Sonde v2 全量备份。");
      }
      if (record.data.backupType !== BACKUP_TYPE || record.data.formatVersion !== FORMAT_VERSION) {
        throw new Error(
          `不支持的备份格式：${record.data.backupType ?? "unknown"} v${record.data.formatVersion ?? "unknown"}`,
        );
      }
      setRestoreFile(file);
      setManifest(record.data);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "无法读取备份 manifest。");
    }
  };

  const handleRestore = async () => {
    if (!restoreFile || !manifest) return;
    if (!window.confirm("确认恢复此全量备份？恢复过程会合并归档中的系统数据，已存在的主键记录不会重复写入。")) {
      return;
    }

    setRestoring(true);
    setError("");
    setSuccess("");
    try {
      const result = await api<RestoreResponse>("/api/v1/admin/system/restore/v2", {
        method: "POST",
        headers: { "content-type": "application/x-ndjson" },
        body: restoreFile,
      });
      setSuccess(`恢复完成：已处理 ${result.restoredRecords.toLocaleString()} 条记录。`);
      setRestoreFile(null);
      setManifest(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "恢复失败。");
    } finally {
      setRestoring(false);
    }
  };

  return (
    <div className="page enter-page">
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 mb-5">
        <div>
          <h2 className="text-xl font-bold tracking-tight text-[var(--text)] m-0">备份与恢复</h2>
          <p className="text-xs text-[var(--muted)] mt-1 mb-0">
            使用 Sonde Backup v2 流式归档迁移完整实例，避免大数据量下整库 JSON 占用内存。
          </p>
        </div>
        <span className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-full text-xs font-semibold bg-[var(--signal-subtle)] text-[var(--signal)] border border-[var(--signal)]/30">
          <HardDrive size={13} />
          NDJSON v2
        </span>
      </div>

      {error ? <div className="form-error mb-4">{error}</div> : null}
      {success ? (
        <div className="mb-4 flex items-center gap-2 rounded-xl border border-[var(--signal)]/30 bg-[var(--signal-subtle)] px-4 py-3 text-xs font-semibold text-[var(--signal)]">
          <CheckCircle2 size={16} />
          {success}
        </div>
      ) : null}

      <div className="grid grid-cols-1 xl:grid-cols-2 gap-5">
        <section className="glass-panel p-6 flex flex-col gap-5">
          <div className="flex items-start gap-3">
            <div className="p-2.5 rounded-xl icon-squircle-green">
              <Download size={18} />
            </div>
            <div>
              <h3 className="text-base font-bold text-[var(--text)] m-0">导出全量流式备份</h3>
              <p className="text-xs text-[var(--muted)] mt-1 mb-0 leading-relaxed">
                服务端使用一致性数据库快照与主键游标分批读取，数据按 NDJSON 逐条发送，不再构造整库 Vec 或巨大 JSON 响应。
              </p>
            </div>
          </div>

          <div className="rounded-xl border border-[var(--border-soft)] bg-[var(--input-bg)] p-4 text-xs text-[var(--muted)] space-y-2">
            <div className="flex justify-between gap-4">
              <span>格式</span>
              <strong className="font-mono text-[var(--text)]">sonde_full_backup_ndjson / 2.0</strong>
            </div>
            <div className="flex justify-between gap-4">
              <span>完整性</span>
              <strong className="text-[var(--text)]">Footer SHA-256 + record count</strong>
            </div>
            <div className="flex justify-between gap-4">
              <span>运行时状态</span>
              <strong className="text-[var(--text)]">会话 / replay / lease / dirty marker 不导出</strong>
            </div>
          </div>

          <div className="rounded-xl border border-[var(--amber)]/30 bg-[var(--amber)]/10 p-4 text-xs text-[var(--amber)] leading-relaxed flex gap-2.5">
            <ShieldAlert size={16} className="shrink-0 mt-0.5" />
            <span>
              归档包含密码哈希和通知渠道凭据，应按密钥材料级别保护。TOTP seed 当前刻意不导出，恢复到新实例后需要重新绑定 2FA。
            </span>
          </div>

          <div className="mt-auto">
            <button type="button" className="primary-button" style={{ width: "auto" }} onClick={handleExport}>
              <Download size={15} />
              下载 Backup v2
            </button>
          </div>
        </section>

        <section className="glass-panel p-6 flex flex-col gap-5">
          <div className="flex items-start gap-3">
            <div className="p-2.5 rounded-xl icon-squircle-amber">
              <ArchiveRestore size={18} />
            </div>
            <div>
              <h3 className="text-base font-bold text-[var(--text)] m-0">恢复流式备份</h3>
              <p className="text-xs text-[var(--muted)] mt-1 mb-0 leading-relaxed">
                浏览器仅读取首行 manifest。服务端先异步暂存并完整验证归档，再开启单个数据库事务批量恢复，损坏文件不会产生部分写入。
              </p>
            </div>
          </div>

          <label className="field">
            <span>选择 Sonde Backup v2 文件</span>
            <input
              type="file"
              accept=".ndjson,.sonde.ndjson,application/x-ndjson"
              onChange={(event) => void handleFileChange(event)}
              style={{ padding: "8px" }}
            />
          </label>

          {restoreFile && manifest ? (
            <div className="rounded-xl border border-[var(--border-soft)] bg-[var(--input-bg)] p-4 text-xs space-y-2">
              <div className="flex items-center gap-2 pb-2 mb-2 border-b border-[var(--border-soft)]">
                <FileArchive size={15} className="text-[var(--signal)]" />
                <strong className="text-[var(--text)] truncate">{restoreFile.name}</strong>
              </div>
              <SummaryRow label="文件大小" value={formatBytes(restoreFile.size)} />
              <SummaryRow label="格式版本" value={manifest.formatVersion} mono />
              <SummaryRow label="Sonde 版本" value={manifest.serverVersion} mono />
              <SummaryRow label="导出时间" value={new Date(manifest.exportedAt).toLocaleString()} />
              <SummaryRow label="包含敏感凭据" value={manifest.containsSecrets ? "是" : "否"} />
              <SummaryRow label="包含 TOTP seed" value={manifest.totpSecretsIncluded ? "是" : "否"} />
              <SummaryRow label="包含预聚合" value={manifest.generatedRollupsIncluded ? "是" : "否"} />
            </div>
          ) : null}

          <div className="mt-auto">
            <button
              type="button"
              className="secondary-button"
              style={{ width: "auto", color: "var(--amber)" }}
              disabled={!restoreFile || !manifest || restoring}
              onClick={() => void handleRestore()}
            >
              {restoring ? <RefreshCw size={15} className="animate-spin" /> : <Upload size={15} />}
              {restoring ? "校验并恢复中..." : "校验并恢复"}
            </button>
          </div>
        </section>
      </div>
    </div>
  );
}

function SummaryRow({
  label,
  value,
  mono = false,
}: {
  label: string;
  value: string;
  mono?: boolean;
}) {
  return (
    <div className="flex justify-between gap-4">
      <span className="text-[var(--muted)]">{label}</span>
      <strong className={`text-[var(--text)] text-right ${mono ? "font-mono" : ""}`}>{value}</strong>
    </div>
  );
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KiB", "MiB", "GiB", "TiB"];
  let value = bytes / 1024;
  let unit = units[0];
  for (let index = 1; index < units.length && value >= 1024; index += 1) {
    value /= 1024;
    unit = units[index];
  }
  return `${value.toFixed(value >= 10 ? 1 : 2)} ${unit}`;
}
