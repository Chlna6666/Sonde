import { FormEvent, useEffect, useState } from "react";
import { ChevronLeft, ChevronRight, Copy, Eye, Search, SlidersHorizontal, X, FileJson, Check, Activity, Radio, ScrollText } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useSearchParams } from "react-router-dom";
import { motion } from "motion/react";
import { CustomSelect } from "../components/CustomSelect";
import { Modal } from "../components/Modal";
import { api } from "../lib/api";
import "../styles/explorer.css";

type Kind = "events" | "metrics" | "logs";
type Application = { id: string; name: string };
type Environment = { id: string; name: string };
type RecordValue = Record<string, unknown> & { id: string; timestamp: number; attributes: unknown };
type Page = { items: RecordValue[]; page: number; pageSize: number; hasMore: boolean };

export function ExplorerPage() {
  const { t } = useTranslation();
  const [searchParams, setSearchParams] = useSearchParams();
  const initialKind = searchParams.get("kind");
  const [kind, setKind] = useState<Kind>(initialKind === "metrics" || initialKind === "logs" ? initialKind : "events");
  const [applications, setApplications] = useState<Application[]>([]);
  const [environments, setEnvironments] = useState<Environment[]>([]);
  const [applicationId, setApplicationId] = useState(searchParams.get("applicationId") ?? searchParams.get("app") ?? "");
  const [environmentId, setEnvironmentId] = useState("");
  const [text, setText] = useState("");
  const [level, setLevel] = useState("");
  const [page, setPage] = useState(1);
  const [data, setData] = useState<Page | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [inspectRecord, setInspectRecord] = useState<RecordValue | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    void api<Application[]>("/api/v1/admin/applications")
      .then((items) => {
        setApplications(items);
        if (!applicationId && items[0]) setApplicationId(items[0].id);
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

  useEffect(() => {
    if (applicationId && environmentId) void load();
  }, [kind, applicationId, environmentId, page]);

  function showError(cause: unknown) {
    setError(cause instanceof Error ? cause.message : t("common.error"));
  }

  function changeKind(next: Kind) {
    setKind(next);
    setPage(1);
    setSearchParams({ kind: next, applicationId });
  }

  async function load(event?: FormEvent) {
    event?.preventDefault();
    setLoading(true);
    setError("");
    const query = new URLSearchParams({
      applicationId,
      environmentId,
      page: String(page),
      pageSize: "50",
    });
    if (text) query.set("text", text);
    if (kind === "logs" && level) query.set("level", level);
    try {
      setData(await api<Page>(`/api/v1/admin/explorer/${kind}?${query}`));
    } catch (cause) {
      showError(cause);
    } finally {
      setLoading(false);
    }
  }

  const handleCopyJson = (record: RecordValue) => {
    navigator.clipboard.writeText(JSON.stringify(record, null, 2));
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const kinds: { value: Kind; label: string; icon: typeof Activity }[] = [
    { value: "events", label: t("explorer.events"), icon: Activity },
    { value: "metrics", label: t("explorer.metrics"), icon: Radio },
    { value: "logs", label: t("explorer.logs"), icon: ScrollText },
  ];

  return (
    <div className="space-y-6">
      {/* Header & Apple Segmented Control */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
        <div>
          <span className="eyebrow">{t("explorer.eyebrow")}</span>
          <h1 className="text-2xl sm:text-3xl font-extrabold tracking-tight text-[var(--text)] m-0 mt-0.5">
            {t("explorer.title")}
          </h1>
          <p className="text-xs text-[var(--muted)] m-0 mt-1 max-w-xl">
            {t("explorer.subtitle")}
          </p>
        </div>

        <div className="segmented-control self-start sm:self-auto">
          {kinds.map((item) => {
            const active = kind === item.value;
            const Icon = item.icon;
            return (
              <button
                key={item.value}
                type="button"
                className={`segmented-control-item flex items-center gap-2 ${active ? "active" : ""}`}
                onClick={() => changeKind(item.value)}
              >
                {active ? (
                  <motion.div
                    layoutId="explorer-kind-pill"
                    transition={{ type: "spring", stiffness: 450, damping: 32 }}
                    className="segmented-control-pill"
                  />
                ) : null}
                <span className="relative z-10 flex items-center gap-1.5">
                  <Icon size={14} />
                  <span>{item.label}</span>
                </span>
              </button>
            );
          })}
        </div>
      </div>

      {/* Glassmorphic Filter Bar */}
      <form onSubmit={(event) => void load(event)} className="glass-panel p-4 flex flex-wrap items-end gap-3.5">
        <div className="flex-1 min-w-[160px]">
          <span className="text-[11px] font-bold text-[var(--muted)] uppercase tracking-wider block mb-1.5">
            {t("migration.application")}
          </span>
          <CustomSelect
            label={t("migration.application")}
            value={applicationId}
            options={applications.map((item) => ({ value: item.id, label: item.name }))}
            onChange={(value) => {
              setApplicationId(value);
              setPage(1);
            }}
          />
        </div>

        <div className="flex-1 min-w-[160px]">
          <span className="text-[11px] font-bold text-[var(--muted)] uppercase tracking-wider block mb-1.5">
            {t("migration.environment")}
          </span>
          <CustomSelect
            label={t("migration.environment")}
            value={environmentId}
            options={environments.map((item) => ({ value: item.id, label: item.name }))}
            onChange={(value) => {
              setEnvironmentId(value);
              setPage(1);
            }}
          />
        </div>

        <div className="flex-2 min-w-[200px]">
          <span className="text-[11px] font-bold text-[var(--muted)] uppercase tracking-wider block mb-1.5">
            {t("explorer.search")}
          </span>
          <div className="relative">
            <Search size={14} className="absolute left-3.5 top-1/2 -translate-y-1/2 text-[var(--faint)]" />
            <input
              type="text"
              value={text}
              onChange={(event) => setText(event.target.value)}
              placeholder={t("explorer.searchHint")}
              className="w-full h-9 pl-9 pr-3 text-xs rounded-xl border border-[var(--border)] bg-[var(--input-bg)] text-[var(--text)] focus:border-[var(--signal)] focus:outline-none"
            />
          </div>
        </div>

        {kind === "logs" ? (
          <div className="min-w-[120px]">
            <span className="text-[11px] font-bold text-[var(--muted)] uppercase tracking-wider block mb-1.5">
              {t("explorer.level")}
            </span>
            <CustomSelect
              label={t("explorer.level")}
              value={level}
              options={[
                { value: "", label: "All Levels" },
                ...["trace", "debug", "info", "warn", "error", "fatal"].map((item) => ({
                  value: item,
                  label: item.toUpperCase(),
                })),
              ]}
              onChange={setLevel}
            />
          </div>
        ) : null}

        <button type="submit" className="primary-button compact h-9 px-4 font-bold flex-shrink-0">
          <Search size={14} />
          <span>{t("explorer.apply")}</span>
        </button>
      </form>

      {error ? (
        <div className="p-4 rounded-2xl bg-[var(--danger-subtle)] border border-[var(--danger)]/30 text-[var(--danger)] text-xs font-semibold">
          {error}
        </div>
      ) : null}

      {/* Results Glass Table */}
      <section className="glass-panel overflow-hidden" aria-busy={loading}>
        <div className="overflow-x-auto">
          <table className="w-full text-left border-collapse text-xs">
            <thead>
              <tr className="border-b border-[var(--border)] bg-[var(--panel-strong)]/60 text-[11px] font-bold text-[var(--muted)] uppercase tracking-wider">
                {columns(kind).map((column) => (
                  <th key={column} className="py-3 px-4">{column}</th>
                ))}
                <th className="py-3 px-4 text-right">Inspect</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-[var(--border-soft)] font-mono text-[11px]">
              {data?.items.map((record) => (
                <tr key={record.id} className="hover:bg-[var(--panel-hover)] transition-colors">
                  <td className="py-3 px-4 text-[var(--muted)] whitespace-nowrap">
                    {new Date(record.timestamp).toLocaleString()}
                  </td>
                  {kind === "events" ? (
                    <>
                      <td className="py-3 px-4 font-bold text-[var(--text)] font-sans">{String(record.name ?? "-")}</td>
                      <td className="py-3 px-4 text-[var(--muted)]">{String(record.deviceId ?? "-")}</td>
                      <td className="py-3 px-4 text-[var(--muted)]">{String(record.userId ?? "-")}</td>
                    </>
                  ) : null}
                  {kind === "metrics" ? (
                    <>
                      <td className="py-3 px-4 font-bold text-[var(--text)] font-sans">{String(record.name ?? "-")}</td>
                      <td className="py-3 px-4 text-[var(--signal)] font-bold">{String(record.value ?? "-")}</td>
                      <td className="py-3 px-4 text-[var(--muted)]">{String(record.unit ?? "-")}</td>
                    </>
                  ) : null}
                  {kind === "logs" ? (
                    <>
                      <td className="py-3 px-4">
                        <span className={`px-2 py-0.5 rounded-full text-[10px] font-bold ${
                          String(record.level).toLowerCase() === "error" || String(record.level).toLowerCase() === "fatal"
                            ? "bg-[var(--danger-subtle)] text-[var(--danger)]"
                            : String(record.level).toLowerCase() === "warn"
                            ? "bg-[var(--amber-subtle)] text-[var(--amber)]"
                            : "bg-[var(--blue-subtle)] text-[var(--blue)]"
                        }`}>
                          {String(record.level ?? "-").toUpperCase()}
                        </span>
                      </td>
                      <td className="py-3 px-4 font-sans text-[var(--text)] max-w-md truncate">{String(record.message ?? "-")}</td>
                      <td className="py-3 px-4 text-[var(--muted)]">{String(record.target ?? "-")}</td>
                    </>
                  ) : null}
                  <td className="py-3 px-4 text-right">
                    <button
                      type="button"
                      className="p-1.5 rounded-lg text-[var(--muted)] hover:text-[var(--signal)] hover:bg-[var(--signal-subtle)] transition-colors cursor-pointer"
                      onClick={() => setInspectRecord(record)}
                      title="Inspect Payload"
                    >
                      <Eye size={14} />
                    </button>
                  </td>
                </tr>
              ))}

              {data?.items.length === 0 && !loading ? (
                <tr>
                  <td colSpan={6} className="py-12 text-center text-xs text-[var(--muted)] font-sans">
                    {t("explorer.empty")}
                  </td>
                </tr>
              ) : null}
            </tbody>
          </table>
        </div>

        {/* Pagination Bar */}
        <div className="flex items-center justify-between px-4 py-3 border-t border-[var(--border-soft)] bg-[var(--panel-strong)]/30 text-xs">
          <span className="text-[var(--muted)]">
            Page <strong className="text-[var(--text)]">{page}</strong>
          </span>
          <div className="flex items-center gap-2">
            <button
              type="button"
              disabled={page <= 1 || loading}
              onClick={() => setPage((p) => Math.max(1, p - 1))}
              className="p-1.5 rounded-lg border border-[var(--border)] text-[var(--text)] disabled:opacity-40 hover:bg-[var(--panel-hover)] transition-all cursor-pointer"
            >
              <ChevronLeft size={14} />
            </button>
            <button
              type="button"
              disabled={!data?.hasMore || loading}
              onClick={() => setPage((p) => p + 1)}
              className="p-1.5 rounded-lg border border-[var(--border)] text-[var(--text)] disabled:opacity-40 hover:bg-[var(--panel-hover)] transition-all cursor-pointer"
            >
              <ChevronRight size={14} />
            </button>
          </div>
        </div>
      </section>

      {/* JSON Inspector Modal */}
      {inspectRecord ? (
        <Modal
          isOpen={true}
          onClose={() => setInspectRecord(null)}
          title="Telemetry Record Inspector"
          subtitle={`ID: ${inspectRecord.id}`}
          icon={<FileJson size={18} />}
          size="lg"
          actions={
            <button
              type="button"
              onClick={() => handleCopyJson(inspectRecord)}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-xl bg-[var(--panel-strong)] border border-[var(--border)] text-xs font-semibold text-[var(--text)] hover:bg-[var(--panel-hover)] cursor-pointer"
            >
              {copied ? <Check size={14} className="text-[var(--signal)]" /> : <Copy size={14} />}
              <span>{copied ? "Copied!" : "Copy JSON"}</span>
            </button>
          }
        >
          <pre className="p-4 rounded-2xl bg-black/50 border border-[var(--border)] text-xs font-mono text-[var(--text)] overflow-x-auto max-h-[60vh] leading-relaxed">
            {JSON.stringify(inspectRecord, null, 2)}
          </pre>
        </Modal>
      ) : null}
    </div>
  );
}

function columns(kind: Kind): string[] {
  switch (kind) {
    case "events":
      return ["Timestamp", "Event Name", "Device ID", "User ID"];
    case "metrics":
      return ["Timestamp", "Metric Name", "Value", "Unit"];
    case "logs":
      return ["Timestamp", "Level", "Message", "Target"];
  }
}
