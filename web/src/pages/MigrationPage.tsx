import { ChangeEvent, FormEvent, useEffect, useState } from "react";
import {
  CheckCircle2,
  Database,
  DatabaseZap,
  Download,
  FileUp,
  RefreshCw,
  ShieldCheck,
  Upload,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { motion } from "motion/react";
import { CustomSelect } from "../components/CustomSelect";
import { Button, Card, Badge, Input, EmptyState } from "../components/ui";
import { api } from "../lib/api";
import "../styles/migration.css";

type Application = { id: string; name: string; slug: string };
type Environment = { id: string; applicationId: string; name: string; slug: string };
type Preview = {
  sourceHash: string;
  rows: number;
  valid: number;
  duplicates: number;
  rejected: number;
  firstDay?: string;
  lastDay?: string;
  appVersions: Record<string, number>;
  launcherVersions: Record<string, number>;
  operatingSystems: Record<string, number>;
};
type ImportRun = {
  id: string;
  sourceHash: string;
  applicationId: string;
  environmentId: string;
  status: string;
  inserted: number;
  deduped: number;
  rejected: number;
  createdAt: number;
};
type ImportResult = { run: ImportRun; alreadyImported: boolean };

export function MigrationPage() {
  const { t } = useTranslation();
  const [activeTab, setActiveTab] = useState<"d1" | "app">("d1");
  const [applications, setApplications] = useState<Application[]>([]);
  const [environments, setEnvironments] = useState<Environment[]>([]);
  const [applicationId, setApplicationId] = useState("");
  const [environmentId, setEnvironmentId] = useState("");
  const [sql, setSql] = useState("");
  const [fileName, setFileName] = useState("");
  const [preview, setPreview] = useState<Preview | null>(null);
  const [runs, setRuns] = useState<ImportRun[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [result, setResult] = useState<ImportResult | null>(null);
  const [selectedExportAppId, setSelectedExportAppId] = useState("");
  const [appImportFile, setAppImportFile] = useState<unknown | null>(null);
  const [appImportFileName, setAppImportFileName] = useState("");
  const [appImportSuccess, setAppImportSuccess] = useState("");

  useEffect(() => {
    void Promise.all([
      api<Application[]>("/api/v1/admin/applications"),
      api<ImportRun[]>("/api/v1/admin/migrations/runs"),
    ])
      .then(([apps, history]) => {
        setApplications(apps);
        setRuns(history);
        if (apps[0]) {
          setApplicationId(apps[0].id);
          setSelectedExportAppId(apps[0].id);
        }
      })
      .catch(showError);
  }, []);

  useEffect(() => {
    if (!applicationId) return;
    void api<Environment[]>(`/api/v1/admin/applications/${applicationId}/environments`)
      .then((items) => {
        setEnvironments(items);
        setEnvironmentId(items[0]?.id ?? "");
      })
      .catch(showError);
  }, [applicationId]);

  function showError(cause: unknown) {
    setError(cause instanceof Error ? cause.message : t("common.error"));
  }

  async function chooseFile(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    if (!file) return;
    if (file.size > 900_000) return setError(t("migration.tooLarge"));
    setSql(await file.text());
    setFileName(file.name);
    setPreview(null);
    setResult(null);
    setError("");
  }

  async function inspect(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError("");
    setResult(null);
    try {
      setPreview(
        await api<Preview>("/api/v1/admin/migrations/d1/preview", {
          method: "POST",
          body: JSON.stringify({ sql }),
        }),
      );
    } catch (cause) {
      showError(cause);
    } finally {
      setBusy(false);
    }
  }

  async function execute() {
    if (!preview || !applicationId || !environmentId) return;
    setBusy(true);
    setError("");
    try {
      const imported = await api<ImportResult>("/api/v1/admin/migrations/d1/execute", {
        method: "POST",
        body: JSON.stringify({ sql, applicationId, environmentId }),
      });
      setResult(imported);
      setRuns((current) => [imported.run, ...current.filter((run) => run.id !== imported.run.id)]);
    } catch (cause) {
      showError(cause);
    } finally {
      setBusy(false);
    }
  }

  const handleExportSingleApp = async () => {
    if (!selectedExportAppId) return;
    const targetApp = applications.find((application) => application.id === selectedExportAppId);
    if (!targetApp) return;
    try {
      const data = await api<unknown>(`/api/v1/admin/applications/${targetApp.id}/export`);
      const blob = new Blob([JSON.stringify(data, null, 2)], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const link = document.createElement("a");
      link.href = url;
      link.download = `${targetApp.slug}-export.sonde.json`;
      document.body.appendChild(link);
      link.click();
      document.body.removeChild(link);
      URL.revokeObjectURL(url);
    } catch (cause) {
      showError(cause);
    }
  };

  const handleAppImportFile = (event: ChangeEvent<HTMLInputElement>) => {
    setError("");
    setAppImportSuccess("");
    const file = event.target.files?.[0];
    if (!file) return;
    setAppImportFileName(file.name);
    const reader = new FileReader();
    reader.onload = (loadEvent) => {
      try {
        const text = loadEvent.target?.result as string;
        const parsed = JSON.parse(text);
        if (!parsed.application) throw new Error("Invalid Sonde application package");
        setAppImportFile(parsed);
      } catch (cause) {
        showError(cause);
        setAppImportFile(null);
      }
    };
    reader.readAsText(file);
  };

  const handleExecuteAppImport = async () => {
    if (!appImportFile) return;
    setBusy(true);
    setError("");
    try {
      await api("/api/v1/admin/applications/import", {
        method: "POST",
        body: JSON.stringify(appImportFile),
      });
      setAppImportSuccess(t("apps.importSuccess"));
      setAppImportFile(null);
      setAppImportFileName("");
      setApplications(await api<Application[]>("/api/v1/admin/applications"));
    } catch (cause) {
      showError(cause);
    } finally {
      setBusy(false);
    }
  };

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const appPreviewData = (appImportFile as any)?.application;

  return (
    <div className="page enter-page">
      <header className="page-header">
        <div>
          <span className="eyebrow">{t("migration.eyebrow")}</span>
          <h1>{t("migration.title")}</h1>
          <p>{t("migration.subtitle")}</p>
        </div>
      </header>

      <div className="segmented-control mb-6 self-start">
        <button
          type="button"
          className={`segmented-control-item flex items-center gap-2 ${activeTab === "d1" ? "active" : ""}`}
          onClick={() => setActiveTab("d1")}
        >
          {activeTab === "d1" ? (
            <motion.div
              layoutId="migration-tab-pill"
              transition={{ type: "spring", stiffness: 450, damping: 32 }}
              className="segmented-control-pill"
            />
          ) : null}
          <span className="relative z-10 flex items-center gap-2">
            <DatabaseZap size={14} />
            <span>{t("migration.tabD1")}</span>
          </span>
        </button>

        <button
          type="button"
          className={`segmented-control-item flex items-center gap-2 ${activeTab === "app" ? "active" : ""}`}
          onClick={() => setActiveTab("app")}
        >
          {activeTab === "app" ? (
            <motion.div
              layoutId="migration-tab-pill"
              transition={{ type: "spring", stiffness: 450, damping: 32 }}
              className="segmented-control-pill"
            />
          ) : null}
          <span className="relative z-10 flex items-center gap-2">
            <Database size={14} />
            <span>{t("migration.tabApp")}</span>
          </span>
        </button>
      </div>

      {error ? <p className="form-error mb-4" role="alert">{error}</p> : null}

      {activeTab === "d1" ? (
        <div className="grid grid-cols-1 xl:grid-cols-2 gap-5">
          <Card className="p-5 flex flex-col gap-4">
            <form onSubmit={inspect} className="flex flex-col gap-4">
              <div className="flex items-start gap-3">
                <div className="p-1.5 rounded-[var(--radius-sm)] icon-squircle-cyan">
                  <FileUp size={18} />
                </div>
                <div>
                  <h3 className="text-sm sm:text-base font-bold text-[var(--text)] m-0">{t("migration.source")}</h3>
                  <p className="text-xs text-[var(--muted)] m-0 mt-0.5">{t("migration.safe")}</p>
                </div>
              </div>

              <label className="file-drop">
                <input
                  type="file"
                  accept=".sql,text/sql,text/plain"
                  onChange={(event) => void chooseFile(event)}
                />
                <DatabaseZap aria-hidden="true" />
                <strong>{fileName || t("migration.choose")}</strong>
                <span>{t("migration.limit")}</span>
              </label>

              <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                <div className="field">
                  <span>{t("migration.application")}</span>
                  <CustomSelect
                    label={t("migration.application")}
                    value={applicationId}
                    options={[
                      { value: "", label: "—" },
                      ...applications.map((item) => ({ value: item.id, label: item.name })),
                    ]}
                    onChange={setApplicationId}
                  />
                </div>
                <div className="field">
                  <span>{t("migration.environment")}</span>
                  <CustomSelect
                    label={t("migration.environment")}
                    value={environmentId}
                    options={[
                      { value: "", label: "—" },
                      ...environments.map((item) => ({ value: item.id, label: item.name })),
                    ]}
                    onChange={setEnvironmentId}
                  />
                </div>
              </div>

              <Button
                type="submit"
                size="sm"
                disabled={!sql || busy || !environmentId}
                loading={busy}
              >
                {t("migration.preview")}
              </Button>
            </form>
          </Card>

          <Card className="p-5 flex flex-col gap-4" aria-live="polite">
            <div className="flex items-start gap-3">
              <div className="p-1.5 rounded-[var(--radius-sm)] icon-squircle-green">
                <ShieldCheck size={18} />
              </div>
              <div>
                <h3 className="text-sm sm:text-base font-bold text-[var(--text)] m-0">{t("migration.report")}</h3>
                <p className="text-xs text-[var(--muted)] m-0 mt-0.5">{t("migration.reportHint")}</p>
              </div>
            </div>

            {preview ? (
              <>
                <dl className="migration-stats">
                  <div><dt>{t("migration.rows")}</dt><dd>{preview.rows.toLocaleString()}</dd></div>
                  <div><dt>{t("migration.valid")}</dt><dd>{preview.valid.toLocaleString()}</dd></div>
                  <div><dt>{t("migration.duplicates")}</dt><dd>{preview.duplicates.toLocaleString()}</dd></div>
                  <div><dt>{t("migration.rejected")}</dt><dd>{preview.rejected.toLocaleString()}</dd></div>
                </dl>
                <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                  <div>
                    <strong className="text-xs font-bold text-[var(--text)] block mb-1">{t("apps.statsAppVersions")}</strong>
                    <ul className="text-xs space-y-1 p-0 m-0 list-none">
                      {Object.entries(preview.appVersions).map(([version, count]) => (
                        <li key={version} className="flex justify-between font-mono text-[11px]">
                          <code>{version}</code> <span>{count.toLocaleString()}</span>
                        </li>
                      ))}
                    </ul>
                  </div>
                  <div>
                    <strong className="text-xs font-bold text-[var(--text)] block mb-1">{t("apps.statsOS")}</strong>
                    <ul className="text-xs space-y-1 p-0 m-0 list-none">
                      {Object.entries(preview.operatingSystems).map(([os, count]) => (
                        <li key={os} className="flex justify-between font-mono text-[11px]">
                          <code>{os}</code> <span>{count.toLocaleString()}</span>
                        </li>
                      ))}
                    </ul>
                  </div>
                </div>
                <Button
                  size="sm"
                  disabled={busy || preview.valid === 0}
                  loading={busy}
                  onClick={() => void execute()}
                >
                  {t("migration.execute")}
                </Button>
              </>
            ) : result ? (
              <div className="flex flex-col items-center justify-center p-6 text-center rounded-[var(--radius-lg)] bg-[var(--signal-subtle)] border border-[var(--signal)]/30">
                <CheckCircle2 size={32} className="text-[var(--signal)] mb-2" />
                <h3 className="text-sm font-bold text-[var(--text)] m-0">{result.alreadyImported ? t("migration.already") : t("migration.done")}</h3>
                <p className="text-xs text-[var(--muted)] m-0 mt-1">
                  {t("migration.insertedDeduped", {
                    inserted: result.run.inserted.toLocaleString(),
                    deduped: result.run.deduped.toLocaleString(),
                  })}
                </p>
              </div>
            ) : (
              <EmptyState
                icon={<DatabaseZap size={24} />}
                title={t("migration.awaiting")}
              />
            )}
          </Card>
        </div>
      ) : null}

      {activeTab === "app" ? (
        <div className="grid grid-cols-1 xl:grid-cols-2 gap-5">
          <Card className="p-5 flex flex-col gap-4">
            <div className="flex items-start gap-3">
              <div className="p-1.5 rounded-[var(--radius-sm)] icon-squircle-green">
                <Download size={18} />
              </div>
              <div>
                <h3 className="text-sm sm:text-base font-bold text-[var(--text)] m-0">{t("apps.exportApp")}</h3>
                <p className="text-xs text-[var(--muted)] m-0 mt-0.5">{t("migration.exportAppDesc")}</p>
              </div>
            </div>

            <div className="field">
              <span>{t("migration.application")}</span>
              <CustomSelect
                label={t("migration.application")}
                value={selectedExportAppId}
                options={applications.map((item) => ({ value: item.id, label: item.name }))}
                onChange={setSelectedExportAppId}
              />
            </div>

            <div className="mt-auto">
              <Button
                size="sm"
                disabled={!selectedExportAppId}
                onClick={handleExportSingleApp}
                icon={<Download size={14} />}
              >
                {t("apps.exportApp")}
              </Button>
            </div>
          </Card>

          <Card className="p-5 flex flex-col gap-4">
            <div className="flex items-start gap-3">
              <div className="p-1.5 rounded-[var(--radius-sm)] icon-squircle-amber">
                <Upload size={18} />
              </div>
              <div>
                <h3 className="text-sm sm:text-base font-bold text-[var(--text)] m-0">{t("apps.importApp")}</h3>
                <p className="text-xs text-[var(--muted)] m-0 mt-0.5">{t("apps.importAppSubtitle")}</p>
              </div>
            </div>

            <label className="file-drop">
              <input type="file" accept=".json,.sonde.json" onChange={handleAppImportFile} />
              <Upload aria-hidden="true" />
              <strong>{appImportFileName || t("apps.importSelectFile")}</strong>
            </label>

            {appImportSuccess ? (
              <div className="flex items-center gap-2 p-3 rounded-[var(--radius-lg)] bg-[var(--signal-subtle)] border border-[var(--signal)]/30 text-[var(--signal)] text-xs font-medium">
                <CheckCircle2 size={18} />
                <span>{appImportSuccess}</span>
              </div>
            ) : null}

            {appPreviewData ? (
              <div className="p-3 rounded-[var(--radius-md)] bg-[var(--input-bg)] border border-[var(--border-soft)] text-xs space-y-1">
                <div className="flex justify-between">
                  <span className="text-[var(--muted)]">{t("migration.appName")}:</span>
                  <strong className="text-[var(--text)]">{appPreviewData.name}</strong>
                </div>
                <div className="flex justify-between">
                  <span className="text-[var(--muted)]">{t("migration.appSlug")}:</span>
                  <code className="text-[var(--text)]">{appPreviewData.slug}</code>
                </div>
              </div>
            ) : null}

            <div className="mt-auto">
              <Button
                variant="secondary"
                size="sm"
                disabled={!appImportFile || busy}
                loading={busy}
                onClick={handleExecuteAppImport}
                icon={<Upload size={14} />}
              >
                {t("apps.importApp")}
              </Button>
            </div>
          </Card>
        </div>
      ) : null}
    </div>
  );
}
