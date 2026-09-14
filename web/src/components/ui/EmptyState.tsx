import React from "react";
import { cn } from "../../lib/utils";

export interface EmptyStateProps extends React.HTMLAttributes<HTMLDivElement> {
  icon?: React.ReactNode;
  title: string;
  description?: string;
  action?: React.ReactNode;
}

export function EmptyState({
  icon,
  title,
  description,
  action,
  className,
  ...props
}: EmptyStateProps) {
  return (
    <div
      className={cn(
        "flex flex-col items-center justify-center p-8 sm:p-12 text-center rounded-[var(--radius-xl)] border border-dashed border-[var(--border)] bg-[var(--card)]/50",
        className
      )}
      {...props}
    >
      {icon ? (
        <div className="flex h-12 w-12 items-center justify-center rounded-[var(--radius-md)] bg-[var(--input-bg)] border border-[var(--border)] text-[var(--muted)] mb-3.5">
          {icon}
        </div>
      ) : null}
      <h3 className="text-sm sm:text-base font-bold text-[var(--text)] m-0">{title}</h3>
      {description ? (
        <p className="mt-1 text-xs text-[var(--muted)] max-w-sm leading-relaxed m-0">{description}</p>
      ) : null}
      {action ? <div className="mt-4">{action}</div> : null}
    </div>
  );
}
