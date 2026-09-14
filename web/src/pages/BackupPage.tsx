import { useState } from "react";
import { useTranslation } from "react-i18next";
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
import { Button, Card, Badge, Input } from "../components/ui";

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
const CURRENT_FORMAT_VERSION = "2.1";
const MANIFEST_READ_BYTES = 64 * 1024;

export function BackupPage() {
  const { t } = useTranslation();
  const [restoreFile, setRestoreFile] = useState<File | null>(null);
  const [manifest, setManifest] = useState<BackupManifest | null>(null);
  const [restoring, setRestoring] = useState(false);
  const [error, setError] = useState("");
  const [success, setSuccess] = useState("");

  const handleExport = () => {
    setError("");
    setSuccess("");
    const link = document.createElement("a");
    link.href = "/api/v1/admin/system/backup";
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
      if (file.size === 0) throw new Error(t("backup.fileEmpty"));
      const prefix = await file.slice(0, Math.min(file.size, MANIFEST_READ_BYTES)).text();
      const newline = prefix.indexOf("\n");
      if (newline < 0) throw new Error(t("backup.noManifestLine"));
      const firstLine = prefix.slice(0, newline).replace(/\r$/, "");
      const record = JSON.parse(firstLine) as { type?: string; data?: BackupManifest };
      if (record.type !== "manifest" || !record.data) {
        throw new Error(t("backup.notSondeBackup"));
      }
      if (
        record.data.backupType !== BACKUP_TYPE ||
        record.data.formatVersion !== CURRENT_FORMAT_VERSION
      ) {
        throw new Error(
          t("backup.unsupportedFormat", {
            type: record.data.backupType ?? "unknown",
            version: record.data.formatVersion ?? "unknown",
          }),
        );
      }
      setRestoreFile(file);
      setManifest(record.data);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t("backup.cannotReadManifest"));
    }
  };

  const handleRestore = async () => {
    if (!restoreFile || !manifest) return;
    if (!window.confirm(t("backup.confirmRestore"))) {
      return;
    }

    setRestoring(true);
    setError("");
    setSuccess("");
    try {
      const result = await api<RestoreResponse>("/api/v1/admin/system/restore", {
        method: "POST",
        headers: { "content-type": "application/x-ndjson" },
        body: restoreFile,
      });
      setSuccess(
        t("backup.restoreSuccess", {
          count: result.restoredRecords.toLocaleString(),
        }),
      );
      setRestoreFile(null);
      setManifest(null);
      window.setTimeout(() => window.location.assign("/login"), 900);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t("backup.restoreFailed"));
      setRestoring(false);
    }
  };

  return (
    <div className="page enter-page">
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 mb-5">
        <div>
          <h2 className="text-xl font-bold tracking-tight text-[var(--text)] m-0">{t("backup.title")}</h2>
          <p className="text-xs text-[var(--muted)] mt-1 mb-0">
            {t("backup.subtitle")}
          </p>
        </div>
        <Badge variant="success" dot size="sm">
          <HardDrive size={13} />
          NDJSON {CURRENT_FORMAT_VERSION}
        </Badge>
      </div>

      {error ? <div className="p-4 rounded-[var(--radius-lg)] bg-[var(--danger-subtle)] border border-[var(--danger)]/30 text-[var(--danger)] text-xs font-semibold mb-4">{error}</div> : null}
      {success ? (
        <div className="mb-4 flex items-center gap-2 rounded-[var(--radius-lg)] border border-[var(--signal)]/30 bg-[var(--signal-subtle)] px-4 py-3 text-xs font-semibold text-[var(--signal)]">
          <CheckCircle2 size={16} />
          {success}
        </div>
      ) : null}

      <div className="grid grid-cols-1 xl:grid-cols-2 gap-5">
        <Card className="p-5 flex flex-col gap-5">
          <div className="flex items-start gap-3">
            <div className="p-1.5 rounded-[var(--radius-sm)] icon-squircle-green">
              <Download size={18} />
            </div>
            <div>
              <h3 className="text-sm sm:text-base font-bold text-[var(--text)] m-0">{t("backup.exportTitle")}</h3>
              <p className="text-xs text-[var(--muted)] mt-1 mb-0 leading-relaxed">
                {t("backup.exportDesc")}
              </p>
            </div>
          </div>

          <div className="rounded-[var(--radius-md)] border border-[var(--border-soft)] bg-[var(--input-bg)] p-4 text-xs text-[var(--muted)] space-y-2">
            <div className="flex justify-between gap-4">
              <span>{t("backup.format")}</span>
              <strong className="font-mono text-[var(--text)]">
                {BACKUP_TYPE} / {CURRENT_FORMAT_VERSION}
              </strong>
            </div>
            <div className="flex justify-between gap-4">
              <span>{t("backup.integrity")}</span>
              <strong className="text-[var(--text)]">Footer SHA-256 + record count</strong>
            </div>
            <div className="flex justify-between gap-4">
              <span>{t("backup.runtimeState")}</span>
              <strong className="text-[var(--text)]">{t("backup.runtimeStateDesc")}</strong>
            </div>
          </div>

          <div className="rounded-[var(--radius-md)] border border-[var(--amber)]/30 bg-[var(--amber-subtle)] p-4 text-xs text-[var(--amber)] leading-relaxed flex gap-2.5">
            <ShieldAlert size={16} className="shrink-0 mt-0.5" />
            <span>
              {t("backup.securityNotice")}
            </span>
          </div>

          <div className="mt-auto">
            <Button size="sm" onClick={handleExport} icon={<Download size={14} />}>
              {t("backup.downloadButton")}
            </Button>
          </div>
        </Card>

        <Card className="p-5 flex flex-col gap-5">
          <div className="flex items-start gap-3">
            <div className="p-1.5 rounded-[var(--radius-sm)] icon-squircle-amber">
              <ArchiveRestore size={18} />
            </div>
            <div>
              <h3 className="text-sm sm:text-base font-bold text-[var(--text)] m-0">{t("backup.restoreTitle")}</h3>
              <p className="text-xs text-[var(--muted)] mt-1 mb-0 leading-relaxed">
                {t("backup.restoreDesc")}
              </p>
            </div>
          </div>

          <div className="rounded-[var(--radius-md)] border border-[var(--danger)]/25 bg-[var(--danger-subtle)] p-3 text-xs text-[var(--danger)] leading-relaxed">
            {t("backup.restoreWarning")}
          </div>

          <label className="flex flex-col gap-1.5 text-xs text-[var(--muted)]">
            <span className="font-semibold text-[var(--text)]">{t("backup.selectFile")}</span>
            <Input
              type="file"
              accept=".ndjson,.sonde.ndjson,application/x-ndjson"
              onChange={(event) => void handleFileChange(event)}
            />
          </label>

          {restoreFile && manifest ? (
            <div className="rounded-[var(--radius-md)] border border-[var(--border-soft)] bg-[var(--input-bg)] p-4 text-xs space-y-2">
              <div className="flex items-center gap-2 pb-2 mb-2 border-b border-[var(--border-soft)]">
                <FileArchive size={15} className="text-[var(--signal)]" />
                <strong className="text-[var(--text)] truncate">{restoreFile.name}</strong>
              </div>
              <SummaryRow label={t("backup.fileSize")} value={formatBytes(restoreFile.size)} />
              <SummaryRow label={t("backup.manifestVersion")} value={manifest.formatVersion} mono />
              <SummaryRow label={t("backup.serverVersion")} value={manifest.serverVersion} mono />
              <SummaryRow label={t("backup.exportTime")} value={new Date(manifest.exportedAt).toLocaleString()} />
              <SummaryRow label={t("backup.containsSecrets")} value={manifest.containsSecrets ? t("backup.yes") : t("backup.no")} />
              <SummaryRow label={t("backup.containsTotp")} value={manifest.totpSecretsIncluded ? t("backup.yes") : t("backup.no")} />
              <SummaryRow label={t("backup.containsRollups")} value={manifest.generatedRollupsIncluded ? t("backup.yes") : t("backup.no")} />
            </div>
          ) : null}

          <div className="mt-auto">
            <Button
              variant="danger"
              size="sm"
              disabled={!restoreFile || !manifest || restoring}
              loading={restoring}
              onClick={() => void handleRestore()}
              icon={<Upload size={14} />}
            >
              {restoring ? t("backup.restoring") : t("backup.restoreButton")}
            </Button>
          </div>
        </Card>
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
