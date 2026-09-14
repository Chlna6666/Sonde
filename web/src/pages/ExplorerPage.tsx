import { FormEvent, useEffect, useState } from "react";
import { ChevronLeft, ChevronRight, Copy, Eye, Search, SlidersHorizontal, X, FileJson, Check, Activity, Radio, ScrollText } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useLocation, useSearchParams } from "react-router-dom";
import { motion } from "motion/react";
import { CustomSelect } from "../components/CustomSelect";
import { Modal } from "../components/Modal";
import { Button, Card, Badge, Table, TableHeader, TableBody, TableRow, TableHead, TableCell, Input, EmptyState } from "../components/ui";
import { api } from "../lib/api";
import "../styles/explorer.css";

type Kind = "events" | "metrics" | "logs";
type Application = { id: string; name: string };
type Environment = { id: string; name: string };
type RecordValue = Record<string, unknown> & { id: string; timestamp: number; attributes: unknown };
type Page = { items: RecordValue[]; page: number; pageSize: number; hasMore: boolean };

export function ExplorerPage({ defaultKind }: { defaultKind?: Kind } = {}) {
  const { t } = useTranslation();
  const location = useLocation();
  const [searchParams, setSearchParams] = useSearchParams();
  const isLogsRoute = location.pathname.startsWith("/logs");
  const initialKind = defaultKind ?? (isLogsRoute ? "logs" : (searchParams.get("kind") as Kind | null));
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
      {/* Header & range switcher */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
        <div>
          <span className="eyebrow">{isLogsRoute ? t("logs.eyebrow") : t("explorer.eyebrow")}</span>
          <h1 className="text-2xl sm:text-3xl font-extrabold tracking-tight text-[var(--text)] m-0 mt-0.5">
            {isLogsRoute ? t("logs.title") : t("explorer.title")}
          </h1>
          <p className="text-xs text-[var(--muted)] m-0 mt-1 max-w-xl">
            {isLogsRoute ? t("logs.subtitle") : t("explorer.subtitle")}
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

      {/* Standardized Filter Bar */}
      <Card className="p-4 mb-5">
        <form onSubmit={(event) => void load(event)} className="flex flex-wrap items-end gap-3.5">
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
              <Search size={14} className="absolute left-3 top-1/2 -translate-y-1/2 text-[var(--faint)] z-10" />
              <Input
                type="text"
                value={text}
                onChange={(event) => setText(event.target.value)}
                placeholder={t("explorer.searchHint")}
                className="pl-8"
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
                  { value: "", label: t("explorer.allLevels") },
                  ...["trace", "debug", "info", "warn", "error", "fatal"].map((item) => ({
                    value: item,
                    label: item.toUpperCase(),
                  })),
                ]}
                onChange={setLevel}
              />
            </div>
          ) : null}

          <Button type="submit" size="default" icon={<Search size={14} />}>
            {t("explorer.apply")}
          </Button>
        </form>

        {kind === "logs" ? (
          <div className="flex flex-wrap items-center gap-1.5 pt-3.5 mt-3.5 border-t border-[var(--card-border)]">
            <span className="text-[11px] font-bold text-[var(--muted)] uppercase tracking-wider mr-1">
              {t("explorer.level")}:
            </span>
            {[
              { value: "", label: t("explorer.allLevels") },
              { value: "fatal", label: "FATAL" },
              { value: "error", label: "ERROR" },
              { value: "warn", label: "WARN" },
              { value: "info", label: "INFO" },
              { value: "debug", label: "DEBUG" },
              { value: "trace", label: "TRACE" },
            ].map((item) => {
              const isSelected = level === item.value;
              return (
                <button
                  key={item.value}
                  type="button"
                  onClick={() => {
                    setLevel(item.value);
                    setPage(1);
                  }}
                  className={`text-[11px] px-2.5 py-0.5 rounded-[var(--radius-sm)] font-semibold transition-colors cursor-pointer ${
                    isSelected
                      ? "bg-[var(--signal)] text-[var(--signal-ink)]"
                      : "bg-[var(--input-bg)] text-[var(--muted)] border border-[var(--card-border)] hover:border-[var(--signal)]/50"
                  }`}
                >
                  {item.label}
                </button>
              );
            })}
          </div>
        ) : null}
      </Card>

      {error ? (
        <div className="p-4 rounded-[var(--radius-lg)] bg-[var(--danger-subtle)] border border-[var(--danger)]/30 text-[var(--danger)] text-xs font-semibold mb-4">
          {error}
        </div>
      ) : null}

      {/* Results Table */}
      <section aria-busy={loading}>
        <Table>
          <TableHeader>
            <TableRow>
              {columns(kind, t).map((column) => (
                <TableHead key={column}>{column}</TableHead>
              ))}
              <TableHead className="text-right">{t("common.inspect")}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody className="font-mono text-[11px]">
            {data?.items.map((record) => (
              <TableRow key={record.id}>
                <TableCell className="text-[var(--muted)] whitespace-nowrap">
                  {new Date(record.timestamp).toLocaleString()}
                </TableCell>
                {kind === "events" ? (
                  <>
                    <TableCell className="font-bold text-[var(--text)] font-sans">{String(record.name ?? "-")}</TableCell>
                    <TableCell className="text-[var(--muted)]">{String(record.deviceId ?? "-")}</TableCell>
                    <TableCell className="text-[var(--muted)]">{String(record.userId ?? "-")}</TableCell>
                  </>
                ) : null}
                {kind === "metrics" ? (
                  <>
                    <TableCell className="font-bold text-[var(--text)] font-sans">{String(record.name ?? "-")}</TableCell>
                    <TableCell className="text-[var(--signal)] font-bold">{String(record.value ?? "-")}</TableCell>
                    <TableCell className="text-[var(--muted)]">{String(record.unit ?? "-")}</TableCell>
                  </>
                ) : null}
                {kind === "logs" ? (
                  <>
                    <TableCell>
                      <Badge
                        variant={
                          String(record.level).toLowerCase() === "error" || String(record.level).toLowerCase() === "fatal"
                            ? "danger"
                            : String(record.level).toLowerCase() === "warn"
                            ? "warning"
                            : "info"
                        }
                        size="sm"
                      >
                        {String(record.level ?? "-").toUpperCase()}
                      </Badge>
                    </TableCell>
                    <TableCell className="font-sans text-[var(--text)] max-w-md truncate">{String(record.message ?? "-")}</TableCell>
                    <TableCell className="text-[var(--muted)]">{String(record.target ?? "-")}</TableCell>
                  </>
                ) : null}
                <TableCell className="text-right">
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    onClick={() => setInspectRecord(record)}
                    icon={<Eye size={14} />}
                    title="Inspect Payload"
                  />
                </TableCell>
              </TableRow>
            ))}

            {data?.items.length === 0 && !loading ? (
              <TableRow>
                <TableCell colSpan={6} className="py-12 text-center text-xs text-[var(--muted)] font-sans">
                  {t("explorer.empty")}
                </TableCell>
              </TableRow>
            ) : null}
          </TableBody>
        </Table>

        {/* Pagination Bar */}
        <div className="flex items-center justify-between px-4 py-3 border border-t-0 border-[var(--card-border)] rounded-b-[var(--radius-lg)] bg-[var(--card)] text-xs">
          <span className="text-[var(--muted)]">
            {t("settings.page", { page })}
          </span>
          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="icon-sm"
              disabled={page <= 1 || loading}
              onClick={() => setPage((p) => Math.max(1, p - 1))}
              icon={<ChevronLeft size={14} />}
            />
            <Button
              variant="outline"
              size="icon-sm"
              disabled={!data?.hasMore || loading}
              onClick={() => setPage((p) => p + 1)}
              icon={<ChevronRight size={14} />}
            />
          </div>
        </div>
      </section>

      {/* JSON Inspector Modal */}
      {inspectRecord ? (
        <Modal
          isOpen={true}
          onClose={() => setInspectRecord(null)}
          title={t("explorer.recordInspector")}
          subtitle={`ID: ${inspectRecord.id}`}
          icon={<FileJson size={18} />}
          size="lg"
          actions={
            <Button
              variant="secondary"
              size="sm"
              onClick={() => handleCopyJson(inspectRecord)}
              icon={copied ? <Check size={14} className="text-[var(--signal)]" /> : <Copy size={14} />}
            >
              {copied ? t("common.copied") : t("common.copyJson")}
            </Button>
          }
        >
          {inspectRecord.level ? (
            <div className="flex flex-wrap items-center gap-2.5 mb-3.5 p-3 rounded-[var(--radius-md)] bg-[var(--input-bg)] border border-[var(--border)] text-xs">
              <span className="font-semibold text-[var(--text)]">{t("explorer.level")}:</span>
              <Badge
                variant={
                  String(inspectRecord.level).toLowerCase() === "error" || String(inspectRecord.level).toLowerCase() === "fatal"
                    ? "danger"
                    : String(inspectRecord.level).toLowerCase() === "warn"
                    ? "warning"
                    : "info"
                }
                size="sm"
              >
                {String(inspectRecord.level).toUpperCase()}
              </Badge>
              {inspectRecord.logger ? (
                <span className="text-[var(--muted)]">
                  <strong className="text-[var(--text)]">Logger:</strong> {String(inspectRecord.logger)}
                </span>
              ) : null}
              {inspectRecord.traceId ? (
                <span className="text-[var(--muted)] font-mono">
                  <strong className="text-[var(--text)]">Trace:</strong> {String(inspectRecord.traceId)}
                </span>
              ) : null}
              {inspectRecord.spanId ? (
                <span className="text-[var(--muted)] font-mono">
                  <strong className="text-[var(--text)]">Span:</strong> {String(inspectRecord.spanId)}
                </span>
              ) : null}
            </div>
          ) : null}
          <pre className="p-4 rounded-[var(--radius-md)] bg-[var(--bg)] border border-[var(--border)] text-xs font-mono text-[var(--text)] overflow-x-auto max-h-[60vh] leading-relaxed">
            {JSON.stringify(inspectRecord, null, 2)}
          </pre>
        </Modal>
      ) : null}
    </div>
  );
}

function columns(kind: Kind, t: (key: string) => string): string[] {
  switch (kind) {
    case "events":
      return [t("explorer.time"), t("explorer.event"), t("explorer.deviceId"), t("explorer.userId")];
    case "metrics":
      return [t("explorer.time"), t("explorer.metric"), t("explorer.value"), t("explorer.unit")];
    case "logs":
      return [t("explorer.time"), t("explorer.level"), t("explorer.message"), t("explorer.target")];
  }
}
