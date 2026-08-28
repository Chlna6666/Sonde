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
  const accentClasses: Record<MetricAccent, { bg: string; border: string; text: string; iconBg: string }> = {
    green: {
      bg: "var(--signal-subtle)",
      border: "rgba(48, 209, 88, 0.2)",
      text: "var(--signal)",
      iconBg: "icon-squircle-green",
    },
    blue: {
      bg: "var(--blue-subtle)",
      border: "rgba(10, 132, 255, 0.2)",
      text: "var(--blue)",
      iconBg: "icon-squircle-blue",
    },
    indigo: {
      bg: "var(--indigo-subtle)",
      border: "rgba(94, 92, 230, 0.2)",
      text: "var(--indigo)",
      iconBg: "icon-squircle-indigo",
    },
    purple: {
      bg: "var(--purple-subtle)",
      border: "rgba(191, 90, 242, 0.2)",
      text: "var(--purple)",
      iconBg: "icon-squircle-purple",
    },
    cyan: {
      bg: "var(--cyan-subtle)",
      border: "rgba(100, 210, 255, 0.2)",
      text: "var(--cyan)",
      iconBg: "icon-squircle-cyan",
    },
    amber: {
      bg: "var(--amber-subtle)",
      border: "rgba(255, 159, 10, 0.2)",
      text: "var(--amber)",
      iconBg: "icon-squircle-amber",
    },
    red: {
      bg: "var(--danger-subtle)",
      border: "rgba(255, 69, 58, 0.2)",
      text: "var(--danger)",
      iconBg: "icon-squircle-danger",
    },
  };

  const style = accentClasses[accent] ?? accentClasses.blue;
  const formattedValue = typeof value === "number" ? new Intl.NumberFormat().format(value) : value;

  return (
    <motion.article
      whileHover={{ y: -2, transition: { duration: 0.18, ease: [0.16, 1, 0.3, 1] } }}
      className="glass-panel p-5 relative overflow-hidden flex flex-col justify-between"
    >
      <div className="flex items-center justify-between gap-3 mb-3">
        <span className="text-xs font-bold uppercase tracking-wider text-[var(--muted)] truncate">
          {label}
        </span>
        <div className={`p-2 rounded-xl ${style.iconBg}`}>
          <Icon size={18} aria-hidden="true" />
        </div>
      </div>

      <div className="flex items-baseline justify-between gap-2 mt-auto">
        <strong className="text-2xl sm:text-3xl font-extrabold tracking-tight text-[var(--text)] font-mono">
          {formattedValue}
        </strong>

        {trend ? (
          <span
            className={`inline-flex items-center gap-1 text-[11px] font-bold px-2 py-0.5 rounded-full ${
              trend.value >= 0
                ? "bg-[var(--signal-subtle)] text-[var(--signal)] border border-[var(--signal)]/30"
                : "bg-[var(--danger-subtle)] text-[var(--danger)] border border-[var(--danger)]/30"
            }`}
          >
            {trend.value >= 0 ? `+${trend.value}%` : `${trend.value}%`}
          </span>
        ) : null}
      </div>

      {/* Apple Subtle Bottom Glow Ribbon */}
      <div
        className="absolute bottom-0 inset-x-0 h-0.5 opacity-60"
        style={{ background: style.text }}
      />
    </motion.article>
  );
}
