import { FormEvent, useEffect, useState } from "react";
import {
  ChevronLeft,
  ChevronRight,
  Copy,
  Eye,
  Search,
  SlidersHorizontal,
  X,
  FileJson,
  Check,
  CheckCircle2,
  Activity,
  Radio,
  ScrollText,
  Trash2,
  RotateCcw,
  AlertTriangle,
  RefreshCw,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { useLocation, useSearchParams } from "react-router-dom";
import { motion } from "motion/react";
import { CustomSelect } from "../components/CustomSelect";
import { Modal } from "../components/Modal";
import { CleanInvalidModal } from "../components/CleanInvalidModal";
import { Button, Card, Badge, Table, TableHeader, TableBody, TableRow, TableHead, TableCell, Input, EmptyState } from "../components/ui";
import { api } from "../lib/api";
import { useCurrentUser } from "../lib/useCurrentUser";
import {
  resolveInitialAppId,
  setUserPreferredAppId,
  formatAppOptions,
} from "../lib/applicationPreferences";
import "../styles/explorer.css";

type Kind = "events" | "metrics" | "logs";
type Application = { id: string; name: string; ownerUserId?: string | null };
type Environment = { id: string; name: string };
type RecordValue = Record<string, unknown> & { id: string; timestamp: number; attributes: unknown };
type Page = { items: RecordValue[]; page: number; pageSize: number; hasMore: boolean };

export function ExplorerPage({ defaultKind }: { defaultKind?: Kind } = {}) {
  const { t } = useTranslation();
  const user = useCurrentUser();
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
  const [successMsg, setSuccessMsg] = useState("");
  const [inspectRecord, setInspectRecord] = useState<RecordValue | null>(null);
  const [copied, setCopied] = useState(false);

  // Selection & Batch Delete
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [deleting, setDeleting] = useState(false);
  const [confirmDeleteModal, setConfirmDeleteModal] = useState<{ open: boolean; ids: string[]; isSingle?: boolean }>({
    open: false,
    ids: [],
  });

  // Reset Records
  const [resetModalOpen, setResetModalOpen] = useState(false);
  const [resetScope, setResetScope] = useState<"kind" | "all">("kind");
  const [resetting, setResetting] = useState(false);

  // Clean Invalid Data
  const [cleanModalOpen, setCleanModalOpen] = useState(false);

  useEffect(() => {
    void api<Application[]>("/api/v1/admin/applications")
      .then((items) => {
        setApplications(items);
        const explicitId = searchParams.get("applicationId") ?? searchParams.get("app");
        const targetId = resolveInitialAppId(items, user, {
          explicitId,
          pagePrefix: "sonde_explorer_app",
        });
        if (!applicationId || !items.some((a) => a.id === applicationId)) {
          setApplicationId(targetId);
          if (targetId && !explicitId) {
            setSearchParams({ kind, applicationId: targetId });
          }
        }
      })
      .catch(showError);
  }, [user]);

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
    setSelectedIds(new Set());
    if (applicationId && environmentId) void load();
  }, [kind, applicationId, environmentId, page]);

  function showError(cause: unknown) {
    setError(cause instanceof Error ? cause.message : t("common.error"));
  }

  function changeKind(next: Kind) {
    setKind(next);
    setPage(1);
    setSelectedIds(new Set());
    setSearchParams({ kind: next, applicationId });
  }

  async function load(event?: FormEvent) {
    event?.preventDefault();
    setLoading(true);
    setError("");
    setSelectedIds(new Set());
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

  const currentPageIds = data?.items.map((item) => item.id) ?? [];
  const isAllSelected = currentPageIds.length > 0 && currentPageIds.every((id) => selectedIds.has(id));
  const isSomeSelected = currentPageIds.some((id) => selectedIds.has(id)) && !isAllSelected;

  function toggleSelect(id: string) {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  function toggleSelectAll() {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (isAllSelected) {
        for (const id of currentPageIds) next.delete(id);
      } else {
        for (const id of currentPageIds) next.add(id);
      }
      return next;
    });
  }

  async function executeDelete(ids: string[]) {
    if (!applicationId || ids.length === 0) return;
    setDeleting(true);
    setError("");
    try {
      const res = await api<{ deleted: number }>(`/api/v1/admin/explorer/${kind}/delete`, {
        method: "POST",
        body: JSON.stringify({
          applicationId,
          environmentId: environmentId || undefined,
          ids,
        }),
      });
      setSelectedIds((prev) => {
        const next = new Set(prev);
        for (const id of ids) next.delete(id);
        return next;
      });
      setSuccessMsg(t("explorer.deleteSuccess", { count: res.deleted }));
      setTimeout(() => setSuccessMsg(""), 4000);
      setConfirmDeleteModal({ open: false, ids: [] });
      if (inspectRecord && ids.includes(inspectRecord.id)) {
        setInspectRecord(null);
      }
      void load();
    } catch (cause) {
      showError(cause);
    } finally {
      setDeleting(false);
    }
  }

  async function executeReset() {
    if (!applicationId) return;
    setResetting(true);
    setError("");
    try {
      const targetKind = resetScope === "all" ? "all" : kind;
      const res = await api<{ deleted: number }>(`/api/v1/admin/explorer/${targetKind}/reset`, {
        method: "POST",
        body: JSON.stringify({
          applicationId,
          environmentId: environmentId || undefined,
        }),
      });
      setSelectedIds(new Set());
      setSuccessMsg(t("explorer.resetSuccess", { count: res.deleted }));
      setTimeout(() => setSuccessMsg(""), 4000);
      setResetModalOpen(false);
      setPage(1);
      void load();
    } catch (cause) {
      showError(cause);
    } finally {
      setResetting(false);
    }
  }

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

        <div className="flex items-center gap-2">
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

          <Button
            variant="outline"
            size="sm"
            onClick={() => setCleanModalOpen(true)}
            icon={<Trash2 size={14} className="text-[var(--danger)]" />}
            className="text-[var(--danger)] hover:bg-[var(--danger-subtle)] hover:border-[var(--danger)]/40 text-xs font-semibold"
            title={t("explorer.cleanInvalid")}
          >
            {t("explorer.cleanInvalid")}
          </Button>
          <Button
            variant="outline"
            size="sm"
            onClick={() => setResetModalOpen(true)}
            icon={<RotateCcw size={14} className="text-[var(--danger)]" />}
            className="text-[var(--danger)] hover:bg-[var(--danger-subtle)] hover:border-[var(--danger)]/40 text-xs font-semibold"
            title={t("explorer.reset")}
          >
            {t("explorer.reset")}
          </Button>
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
              options={formatAppOptions(applications, user, { myAppLabel: t("overview.myApp") })}
              onChange={(value) => {
                setApplicationId(value);
                setUserPreferredAppId(user, value, "sonde_explorer_app");
                setUserPreferredAppId(user, value, "sonde_preferred_app");
                setPage(1);
                setSearchParams({ kind, applicationId: value });
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

      {successMsg ? (
        <div className="flex items-center gap-2 p-3 mb-4 rounded-xl bg-[var(--signal)]/10 border border-[var(--signal)] text-[var(--signal)] text-xs font-semibold">
          <CheckCircle2 size={16} />
          {successMsg}
        </div>
      ) : null}

      {/* Batch Action Bar */}
      {selectedIds.size > 0 ? (
        <div className="flex items-center justify-between p-3 mb-4 rounded-[var(--radius-lg)] bg-[var(--panel-strong)] border border-[var(--border-highlight)] shadow-sm">
          <div className="flex items-center gap-2 text-xs font-semibold text-[var(--text)]">
            <span className="px-2 py-0.5 rounded-[var(--radius-sm)] bg-[var(--signal)] text-[var(--signal-ink)] font-mono font-bold">
              {selectedIds.size}
            </span>
            <span>{t("explorer.selectedCount", { count: selectedIds.size })}</span>
          </div>
          <div className="flex items-center gap-2">
            <Button
              variant="ghost"
              size="sm"
              onClick={() => setSelectedIds(new Set())}
            >
              {t("explorer.clearSelection")}
            </Button>
            <Button
              variant="danger"
              size="sm"
              onClick={() => setConfirmDeleteModal({ open: true, ids: Array.from(selectedIds) })}
              icon={<Trash2 size={14} />}
            >
              {t("explorer.batchDelete")}
            </Button>
          </div>
        </div>
      ) : null}

      {/* Results Table */}
      <section aria-busy={loading}>
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead className="w-10 text-center">
                <input
                  type="checkbox"
                  checked={isAllSelected}
                  ref={(input) => {
                    if (input) input.indeterminate = isSomeSelected;
                  }}
                  onChange={toggleSelectAll}
                  aria-label="Select all records"
                  className="w-4 h-4 rounded border-[var(--border)] accent-[var(--signal)] cursor-pointer align-middle"
                />
              </TableHead>
              {columns(kind, t).map((column) => (
                <TableHead key={column}>{column}</TableHead>
              ))}
              <TableHead className="text-right">{t("common.actions") || t("common.inspect")}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody className="font-mono text-[11px]">
            {data?.items.map((record) => (
              <TableRow key={record.id} className={selectedIds.has(record.id) ? "bg-[var(--signal)]/5" : ""}>
                <TableCell className="text-center">
                  <input
                    type="checkbox"
                    checked={selectedIds.has(record.id)}
                    onChange={() => toggleSelect(record.id)}
                    aria-label={`Select ${record.id}`}
                    className="w-4 h-4 rounded border-[var(--border)] accent-[var(--signal)] cursor-pointer align-middle"
                  />
                </TableCell>
                <TableCell className="text-[var(--muted)] whitespace-nowrap">
                  {new Date(record.timestamp).toLocaleString()}
                </TableCell>
                {kind === "events" ? (
                  <>
                    <TableCell className="font-bold text-[var(--text)] font-sans">{String(record.name ?? "-")}</TableCell>
                    <TableCell className="text-[var(--muted)]">{String(record.anonymousId ?? record.deviceId ?? "-")}</TableCell>
                    <TableCell className="text-[var(--muted)]">{String(record.appVersion ?? "-")}</TableCell>
                    <TableCell className="text-[var(--muted)]">{String(record.os ?? "-")}</TableCell>
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
                <TableCell className="text-right whitespace-nowrap">
                  <div className="inline-flex items-center gap-1">
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      onClick={() => setInspectRecord(record)}
                      icon={<Eye size={14} />}
                      title={t("explorer.inspectPayload")}
                    />
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      onClick={() => setConfirmDeleteModal({ open: true, ids: [record.id], isSingle: true })}
                      icon={<Trash2 size={14} className="text-[var(--danger)]" />}
                      title={t("explorer.deleteRecord")}
                    />
                  </div>
                </TableCell>
              </TableRow>
            ))}

            {data?.items.length === 0 && !loading ? (
              <TableRow>
                <TableCell colSpan={columns(kind, t).length + 2} className="py-12 text-center text-xs text-[var(--muted)] font-sans">
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
            <div className="flex items-center gap-2">
              <Button
                variant="outline"
                size="sm"
                className="text-[var(--danger)] hover:bg-[var(--danger-subtle)] hover:border-[var(--danger)]/30"
                onClick={() => setConfirmDeleteModal({ open: true, ids: [inspectRecord.id], isSingle: true })}
                icon={<Trash2 size={14} />}
              >
                {t("explorer.deleteRecord")}
              </Button>
              <Button
                variant="secondary"
                size="sm"
                onClick={() => handleCopyJson(inspectRecord)}
                icon={copied ? <Check size={14} className="text-[var(--signal)]" /> : <Copy size={14} />}
              >
                {copied ? t("common.copied") : t("common.copyJson")}
              </Button>
            </div>
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

      {/* Confirm Delete Modal */}
      {confirmDeleteModal.open ? (
        <Modal
          isOpen={true}
          onClose={() => setConfirmDeleteModal({ open: false, ids: [] })}
          title={confirmDeleteModal.isSingle ? t("explorer.deleteRecord") : t("explorer.batchDelete")}
          icon={<Trash2 size={18} className="text-[var(--danger)]" />}
          size="sm"
          actions={
            <div className="flex items-center justify-end gap-2">
              <Button
                variant="secondary"
                size="sm"
                onClick={() => setConfirmDeleteModal({ open: false, ids: [] })}
                disabled={deleting}
              >
                {t("common.cancel")}
              </Button>
              <Button
                variant="danger"
                size="sm"
                onClick={() => void executeDelete(confirmDeleteModal.ids)}
                disabled={deleting}
                icon={deleting ? <RefreshCw size={14} className="animate-spin" /> : <Trash2 size={14} />}
              >
                {deleting ? t("explorer.deleting") : t("common.delete")}
              </Button>
            </div>
          }
        >
          <p className="text-sm text-[var(--muted)] leading-relaxed m-0">
            {confirmDeleteModal.isSingle
              ? t("explorer.confirmDeleteSingle")
              : t("explorer.confirmDeleteSelected", { count: confirmDeleteModal.ids.length })}
          </p>
        </Modal>
      ) : null}

      {/* Reset Records Modal */}
      {resetModalOpen ? (
        <Modal
          isOpen={true}
          onClose={() => setResetModalOpen(false)}
          title={t("explorer.reset")}
          icon={<AlertTriangle size={18} className="text-[var(--danger)]" />}
          size="md"
          actions={
            <div className="flex items-center justify-end gap-2">
              <Button
                variant="secondary"
                size="sm"
                onClick={() => setResetModalOpen(false)}
                disabled={resetting}
              >
                {t("common.cancel")}
              </Button>
              <Button
                variant="danger"
                size="sm"
                onClick={() => void executeReset()}
                disabled={resetting}
                icon={resetting ? <RefreshCw size={14} className="animate-spin" /> : <Trash2 size={14} />}
              >
                {resetting ? t("explorer.resetting") : t("explorer.confirmResetButton")}
              </Button>
            </div>
          }
        >
          <div className="space-y-4">
            <p className="text-xs text-[var(--muted)] m-0">
              {t("explorer.resetPrompt")}
            </p>

            <div className="space-y-2">
              <label className="flex items-center gap-3 p-3 rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--input-bg)] cursor-pointer hover:border-[var(--signal)] transition-colors">
                <input
                  type="radio"
                  name="resetScope"
                  value="kind"
                  checked={resetScope === "kind"}
                  onChange={() => setResetScope("kind")}
                  className="accent-[var(--signal)] cursor-pointer"
                />
                <div>
                  <div className="text-xs font-semibold text-[var(--text)]">
                    {t("explorer.resetScopeKind", { kind: t(`explorer.${kind}`) })}
                  </div>
                </div>
              </label>

              <label className="flex items-center gap-3 p-3 rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--input-bg)] cursor-pointer hover:border-[var(--danger)] transition-colors">
                <input
                  type="radio"
                  name="resetScope"
                  value="all"
                  checked={resetScope === "all"}
                  onChange={() => setResetScope("all")}
                  className="accent-[var(--danger)] cursor-pointer"
                />
                <div>
                  <div className="text-xs font-semibold text-[var(--text)]">
                    {t("explorer.resetScopeAll")}
                  </div>
                </div>
              </label>
            </div>

            <div className="p-3 rounded-[var(--radius-md)] bg-[var(--danger-subtle)] border border-[var(--danger)]/30 text-xs text-[var(--danger)]">
              {t("explorer.confirmResetWarning")}
            </div>
          </div>
        </Modal>
      ) : null}

      {/* Clean Invalid Data Modal */}
      {cleanModalOpen ? (
        <CleanInvalidModal
          isOpen={true}
          onClose={() => setCleanModalOpen(false)}
          applicationId={applicationId}
          appName={applications.find((a) => a.id === applicationId)?.name}
          onSuccess={(deletedCount) => {
            setSelectedIds(new Set());
            setSuccessMsg(t("overview.cleanInvalidSuccess", { count: deletedCount }));
            setTimeout(() => setSuccessMsg(""), 4000);
            setPage(1);
            void load();
          }}
        />
      ) : null}
    </div>
  );
}

function columns(kind: Kind, t: (key: string) => string): string[] {
  switch (kind) {
    case "events":
      return [
        t("explorer.time"),
        t("explorer.event"),
        t("explorer.deviceId"),
        t("explorer.version"),
        t("explorer.os"),
      ];
    case "metrics":
      return [t("explorer.time"), t("explorer.metric"), t("explorer.value"), t("explorer.unit")];
    case "logs":
      return [t("explorer.time"), t("explorer.level"), t("explorer.message"), t("explorer.target")];
  }
}
