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
          <span className="eyebrow">TRANSFER / DATA HUB</span>
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
        <div className="migration-layout">
          <form className="migration-panel" onSubmit={inspect}>
            <div className="panel-heading">
              <FileUp aria-hidden="true" />
              <div>
                <h2>{t("migration.source")}</h2>
                <p>{t("migration.safe")}</p>
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
            <div className="two-columns">
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
            <button className="primary-button" disabled={!sql || busy || !environmentId}>
              {busy ? t("common.loading") : t("migration.preview")}
            </button>
          </form>

          <section className="migration-panel" aria-live="polite">
            <div className="panel-heading">
              <ShieldCheck aria-hidden="true" />
              <div>
                <h2>{t("migration.report")}</h2>
                <p>{t("migration.reportHint")}</p>
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
                <div className="two-columns">
                  <div>
                    <strong>{t("apps.statsAppVersions")}</strong>
                    <ul>
                      {Object.entries(preview.appVersions).map(([version, count]) => (
                        <li key={version}><code>{version}</code> <span>{count.toLocaleString()}</span></li>
                      ))}
                    </ul>
                  </div>
                  <div>
                    <strong>{t("apps.statsOS")}</strong>
                    <ul>
                      {Object.entries(preview.operatingSystems).map(([os, count]) => (
                        <li key={os}><code>{os}</code> <span>{count.toLocaleString()}</span></li>
                      ))}
                    </ul>
                  </div>
                </div>
                <button
                  type="button"
                  className="primary-button"
                  disabled={busy || preview.valid === 0}
                  onClick={() => void execute()}
                >
                  {busy ? t("common.loading") : t("migration.execute")}
                </button>
              </>
            ) : result ? (
              <div className="migration-success">
                <CheckCircle2 size={32} className="text-signal" />
                <h3>{result.alreadyImported ? t("migration.already") : t("migration.done")}</h3>
                <p>
                  Inserted {result.run.inserted.toLocaleString()} events, deduped{" "}
                  {result.run.deduped.toLocaleString()}.
                </p>
              </div>
            ) : (
              <div className="empty-state" style={{ minHeight: "200px" }}>
                <p>{t("migration.awaiting")}</p>
              </div>
            )}
          </section>
        </div>
      ) : null}

      {activeTab === "app" ? (
        <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(320px, 1fr))", gap: "20px" }}>
          <div className="migration-panel">
            <div className="panel-heading">
              <Download aria-hidden="true" className="text-signal" />
              <div>
                <h2>{t("apps.exportApp")}</h2>
                <p>Export a single application package including environments, API keys, and telemetry.</p>
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
            <button
              type="button"
              className="primary-button"
              disabled={!selectedExportAppId}
              onClick={handleExportSingleApp}
            >
              <Download size={15} />
              {t("apps.exportApp")}
            </button>
          </div>

          <div className="migration-panel">
            <div className="panel-heading">
              <Upload aria-hidden="true" className="text-amber" />
              <div>
                <h2>{t("apps.importApp")}</h2>
                <p>{t("apps.importAppSubtitle")}</p>
              </div>
            </div>
            <label className="file-drop">
              <input type="file" accept=".json,.sonde.json" onChange={handleAppImportFile} />
              <Upload aria-hidden="true" />
              <strong>{appImportFileName || t("apps.importSelectFile")}</strong>
            </label>

            {appImportSuccess ? (
              <div className="migration-success" style={{ padding: "12px" }}>
                <CheckCircle2 size={20} className="text-signal" />
                <span>{appImportSuccess}</span>
              </div>
            ) : null}

            {appPreviewData ? (
              <div style={{ background: "var(--input-bg)", padding: "12px", borderRadius: "6px", fontSize: "0.78rem" }}>
                <div className="flex justify-between">
                  <span className="text-muted">App Name:</span>
                  <strong>{appPreviewData.name}</strong>
                </div>
                <div className="flex justify-between">
                  <span className="text-muted">Slug:</span>
                  <code>{appPreviewData.slug}</code>
                </div>
              </div>
            ) : null}

            <button
              type="button"
              className="secondary-button"
              style={{ color: "var(--signal)", borderColor: "var(--signal)" }}
              disabled={!appImportFile || busy}
              onClick={handleExecuteAppImport}
            >
              {busy ? <RefreshCw size={15} className="animate-spin" /> : <Upload size={15} />}
              {busy ? t("common.loading") : t("apps.importApp")}
            </button>
          </div>
        </div>
      ) : null}
    </div>
  );
}
