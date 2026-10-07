import React, { useEffect, useState, useCallback } from "react";
import { useTranslation } from "react-i18next";
import {
  Trash2,
  RefreshCw,
  AlertTriangle,
  CheckCircle2,
  Activity,
  BarChart2,
  FileText,
  Layers,
  Monitor,
} from "lucide-react";
import { api } from "../lib/api";
import { Modal } from "./Modal";
import { Button } from "./ui/Button";
import { Badge } from "./ui/Badge";

export interface InvalidRecordSample {
  kind: string;
  id: string;
  reason: string;
  detail?: string | null;
}

export interface CleanInvalidPreview {
  invalidEvents: number;
  invalidDimensions: number;
  invalidMetrics: number;
  invalidLogs: number;
  invalidDevices: number;
  totalInvalid: number;
  samples: InvalidRecordSample[];
}

export interface CleanInvalidModalProps {
  isOpen: boolean;
  onClose: () => void;
  applicationId?: string;
  appName?: string;
  onSuccess: (deletedCount: number) => void;
}

export function CleanInvalidModal({
  isOpen,
  onClose,
  applicationId,
  appName,
  onSuccess,
}: CleanInvalidModalProps) {
  const { t } = useTranslation();
  const [loadingPreview, setLoadingPreview] = useState(false);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [preview, setPreview] = useState<CleanInvalidPreview | null>(null);
  const [cleaning, setCleaning] = useState(false);

  const effectiveAppId = applicationId && applicationId !== "all" ? applicationId : undefined;

  const loadPreview = useCallback(async () => {
    setLoadingPreview(true);
    setPreviewError(null);
    try {
      const data = await api<CleanInvalidPreview>(
        "/api/v1/admin/explorer/clean-invalid/preview",
        {
          method: "POST",
          body: JSON.stringify({
            applicationId: effectiveAppId,
          }),
        }
      );
      setPreview(data);
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      setPreviewError(msg);
    } finally {
      setLoadingPreview(false);
    }
  }, [effectiveAppId]);

  useEffect(() => {
    if (isOpen) {
      void loadPreview();
    } else {
      setPreview(null);
      setPreviewError(null);
      setCleaning(false);
    }
  }, [isOpen, loadPreview]);

  async function handleConfirmClean() {
    setCleaning(true);
    try {
      const res = await api<{ totalDeleted: number }>(
        "/api/v1/admin/explorer/clean-invalid",
        {
          method: "POST",
          body: JSON.stringify({
            applicationId: effectiveAppId,
          }),
        }
      );
      onSuccess(res.totalDeleted);
      onClose();
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      setPreviewError(msg);
    } finally {
      setCleaning(false);
    }
  }

  function getKindLabel(kind: string): string {
    switch (kind) {
      case "events":
        return t("overview.cleanEvents");
      case "metrics":
        return t("overview.cleanMetrics");
      case "logs":
        return t("overview.cleanLogs");
      case "dimensions":
        return t("overview.cleanDimensions");
      case "devices":
        return t("overview.cleanDevices");
      default:
        return kind;
    }
  }

  return (
    <Modal
      isOpen={isOpen}
      onClose={onClose}
      title={t("overview.cleanInvalidTitle")}
      subtitle={
        appName ? (
          <span>{appName}</span>
        ) : (
          <span>{t("overview.allApps")}</span>
        )
      }
      icon={<Trash2 size={18} className="text-[var(--danger)]" />}
      size="lg"
      actions={
        <div className="flex items-center justify-end gap-2">
          {preview && preview.totalInvalid > 0 ? (
            <>
              <Button
                variant="secondary"
                size="sm"
                onClick={onClose}
                disabled={cleaning}
              >
                {t("common.cancel")}
              </Button>
              <Button
                variant="danger"
                size="sm"
                onClick={() => void handleConfirmClean()}
                disabled={cleaning}
                loading={cleaning}
                icon={<Trash2 size={14} />}
              >
                {cleaning
                  ? t("overview.cleaning")
                  : t("overview.cleanConfirmCount", {
                      count: preview.totalInvalid,
                    })}
              </Button>
            </>
          ) : (
            <Button
              variant="secondary"
              size="sm"
              onClick={onClose}
              disabled={loadingPreview || cleaning}
            >
              {t("overview.cleanClose")}
            </Button>
          )}
        </div>
      }
    >
      <div className="space-y-4">
        {loadingPreview ? (
          <div className="flex flex-col items-center justify-center py-12 text-center">
            <RefreshCw
              size={26}
              className="animate-spin text-[var(--signal)] mb-3"
            />
            <p className="text-sm font-medium text-[var(--text)] m-0">
              {t("overview.cleanScanning")}
            </p>
          </div>
        ) : previewError ? (
          <div className="p-4 rounded-[var(--radius-md)] bg-[var(--danger-subtle)] border border-[var(--danger)]/30 text-[var(--danger)] text-xs flex flex-col gap-2">
            <div className="flex items-center gap-2 font-semibold">
              <AlertTriangle size={16} />
              <span>{t("overview.cleanScanError")}</span>
            </div>
            <p className="m-0 text-[var(--text)]">{previewError}</p>
            <div className="pt-2">
              <Button
                variant="secondary"
                size="sm"
                onClick={() => void loadPreview()}
                icon={<RefreshCw size={13} />}
              >
                {t("overview.cleanRetry")}
              </Button>
            </div>
          </div>
        ) : preview && preview.totalInvalid === 0 ? (
          <div className="flex flex-col items-center justify-center py-8 text-center px-4">
            <div className="w-12 h-12 rounded-full bg-[var(--signal-subtle)] text-[var(--signal)] flex items-center justify-center mb-3">
              <CheckCircle2 size={24} />
            </div>
            <h3 className="text-sm font-bold text-[var(--text)] m-0 mb-1.5">
              {t("overview.cleanNoInvalidTitle")}
            </h3>
            <p className="text-xs text-[var(--muted)] max-w-md m-0 leading-relaxed">
              {t("overview.cleanNoInvalidDesc")}
            </p>
          </div>
        ) : preview && preview.totalInvalid > 0 ? (
          <>
            {/* Warning summary banner */}
            <div className="p-3.5 rounded-[var(--radius-md)] bg-[var(--amber-subtle)] border border-[var(--amber)]/30 text-[var(--text)] text-xs space-y-1.5">
              <div className="flex items-center gap-2 font-bold text-[var(--amber)]">
                <AlertTriangle size={15} />
                <span>
                  {t("overview.cleanFoundSummary", {
                    count: preview.totalInvalid,
                  })}
                </span>
              </div>
              <p className="m-0 text-[11px] text-[var(--muted)] leading-relaxed">
                {t("overview.cleanWarning")}
              </p>
            </div>

            {/* Category breakdown stats */}
            <div className="grid grid-cols-2 sm:grid-cols-5 gap-2">
              <div className="p-2.5 rounded-[var(--radius-md)] bg-[var(--input-bg)] border border-[var(--border)] flex flex-col justify-between">
                <div className="flex items-center gap-1.5 text-[11px] text-[var(--muted)] mb-1">
                  <Activity size={12} className="text-[var(--signal)]" />
                  <span>{t("overview.cleanEvents")}</span>
                </div>
                <div className="text-base font-bold font-mono text-[var(--text)]">
                  {preview.invalidEvents}
                </div>
              </div>

              <div className="p-2.5 rounded-[var(--radius-md)] bg-[var(--input-bg)] border border-[var(--border)] flex flex-col justify-between">
                <div className="flex items-center gap-1.5 text-[11px] text-[var(--muted)] mb-1">
                  <BarChart2 size={12} className="text-[var(--blue)]" />
                  <span>{t("overview.cleanMetrics")}</span>
                </div>
                <div className="text-base font-bold font-mono text-[var(--text)]">
                  {preview.invalidMetrics}
                </div>
              </div>

              <div className="p-2.5 rounded-[var(--radius-md)] bg-[var(--input-bg)] border border-[var(--border)] flex flex-col justify-between">
                <div className="flex items-center gap-1.5 text-[11px] text-[var(--muted)] mb-1">
                  <FileText size={12} className="text-[var(--amber)]" />
                  <span>{t("overview.cleanLogs")}</span>
                </div>
                <div className="text-base font-bold font-mono text-[var(--text)]">
                  {preview.invalidLogs}
                </div>
              </div>

              <div className="p-2.5 rounded-[var(--radius-md)] bg-[var(--input-bg)] border border-[var(--border)] flex flex-col justify-between">
                <div className="flex items-center gap-1.5 text-[11px] text-[var(--muted)] mb-1">
                  <Layers size={12} className="text-[var(--purple)]" />
                  <span>{t("overview.cleanDimensions")}</span>
                </div>
                <div className="text-base font-bold font-mono text-[var(--text)]">
                  {preview.invalidDimensions}
                </div>
              </div>

              <div className="col-span-2 sm:col-span-1 p-2.5 rounded-[var(--radius-md)] bg-[var(--input-bg)] border border-[var(--border)] flex flex-col justify-between">
                <div className="flex items-center gap-1.5 text-[11px] text-[var(--muted)] mb-1">
                  <Monitor size={12} className="text-[var(--danger)]" />
                  <span>{t("overview.cleanDevices")}</span>
                </div>
                <div className="text-base font-bold font-mono text-[var(--text)]">
                  {preview.invalidDevices}
                </div>
              </div>
            </div>

            {/* Samples table */}
            {preview.samples.length > 0 ? (
              <div className="space-y-2 pt-1">
                <div className="text-xs font-semibold text-[var(--text)]">
                  {t("overview.cleanSamplesTitle")}
                </div>
                <div className="border border-[var(--border)] rounded-[var(--radius-md)] overflow-hidden max-h-56 overflow-y-auto">
                  <table className="w-full text-left text-xs border-collapse">
                    <thead className="bg-[var(--input-bg)] border-b border-[var(--border)] sticky top-0 z-10 text-[10px] text-[var(--muted)] uppercase tracking-wider">
                      <tr>
                        <th className="py-1.5 px-3">{t("overview.cleanSampleKind")}</th>
                        <th className="py-1.5 px-3">{t("overview.cleanSampleReason")}</th>
                        <th className="py-1.5 px-3">{t("overview.cleanSampleDetail")}</th>
                      </tr>
                    </thead>
                    <tbody className="divide-y divide-[var(--border)]">
                      {preview.samples.map((sample, idx) => (
                        <tr
                          key={`${sample.kind}-${sample.id}-${idx}`}
                          className="hover:bg-[var(--panel-hover)] transition-colors"
                        >
                          <td className="py-2 px-3 whitespace-nowrap align-top">
                            <Badge variant="outline" size="sm">
                              {getKindLabel(sample.kind)}
                            </Badge>
                          </td>
                          <td className="py-2 px-3 whitespace-nowrap align-top">
                            <span className="inline-block text-[11px] font-medium text-[var(--danger)]">
                              {sample.reason}
                            </span>
                          </td>
                          <td className="py-2 px-3 align-top font-mono text-[11px] text-[var(--muted)] break-all">
                            {sample.detail ? (
                              <div>{sample.detail}</div>
                            ) : null}
                            <div className="text-[10px] text-[var(--muted)] opacity-60">
                              ID: {sample.id}
                            </div>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
            ) : null}
          </>
        ) : null}
      </div>
    </Modal>
  );
}
