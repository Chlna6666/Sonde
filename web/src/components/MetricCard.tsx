import type { LucideIcon } from "lucide-react";
import { motion } from "motion/react";

export type MetricAccent = "green" | "blue" | "indigo" | "purple" | "cyan" | "amber" | "red";

export function MetricCard({
  label,
  value,
  icon: Icon,
  accent = "blue",
  trend,
}: {
  label: string;
  value: number | string;
  icon: LucideIcon;
  accent?: MetricAccent;
  trend?: { value: number; label?: string };
}) {
  const accentClasses: Record<MetricAccent, { text: string; iconBg: string }> = {
    green: { text: "var(--signal)", iconBg: "icon-squircle-green" },
    blue: { text: "var(--blue)", iconBg: "icon-squircle-blue" },
    indigo: { text: "var(--indigo)", iconBg: "icon-squircle-indigo" },
    purple: { text: "var(--purple)", iconBg: "icon-squircle-purple" },
    cyan: { text: "var(--cyan)", iconBg: "icon-squircle-cyan" },
    amber: { text: "var(--amber)", iconBg: "icon-squircle-amber" },
    red: { text: "var(--danger)", iconBg: "icon-squircle-danger" },
  };

  const style = accentClasses[accent] ?? accentClasses.blue;
  const formattedValue = typeof value === "number" ? new Intl.NumberFormat().format(value) : value;

  return (
    <motion.article
      whileHover={{ y: -2, transition: { duration: 0.18, ease: [0.22, 1, 0.36, 1] } }}
      className="glass-panel p-5 relative overflow-hidden flex flex-col justify-between"
    >
      <div className="flex items-center justify-between gap-3 mb-3">
        <span className="text-[11px] font-bold uppercase tracking-[0.12em] text-[var(--muted)] truncate">
          {label}
        </span>
        <div className={`p-1.5 rounded-[var(--radius-sm)] ${style.iconBg}`}>
          <Icon size={16} aria-hidden="true" />
        </div>
      </div>

      <div className="flex items-baseline justify-between gap-2 mt-auto">
        <strong className="text-2xl sm:text-3xl font-bold tracking-tight text-[var(--text)] font-mono">
          {formattedValue}
        </strong>

        {trend ? (
          <span
            className={`inline-flex items-center gap-1 text-[11px] font-bold px-2 py-0.5 rounded-[var(--radius-sm)] ${
              trend.value >= 0
                ? "bg-[var(--signal-subtle)] text-[var(--signal)] border border-[var(--signal)]/30"
                : "bg-[var(--danger-subtle)] text-[var(--danger)] border border-[var(--danger)]/30"
            }`}
          >
            {trend.value >= 0 ? `+${trend.value}%` : `${trend.value}%`}
          </span>
        ) : null}
      </div>

      <div
        className="absolute bottom-0 inset-x-0 h-px opacity-80"
        style={{ background: style.text }}
      />
    </motion.article>
  );
}
