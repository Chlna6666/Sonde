import React from "react";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "../../lib/utils";

export const badgeVariants = cva(
  "inline-flex items-center gap-1.5 font-semibold transition-colors select-none",
  {
    variants: {
      variant: {
        default:
          "bg-[var(--input-bg)] text-[var(--text)] border border-[var(--border-soft)]",
        success:
          "bg-[var(--signal-subtle)] text-[var(--signal)] border border-[var(--signal)]/30",
        danger:
          "bg-[var(--danger-subtle)] text-[var(--danger)] border border-[var(--danger)]/30",
        warning:
          "bg-[var(--amber-subtle)] text-[var(--amber)] border border-[var(--amber)]/30",
        info:
          "bg-[var(--blue-subtle)] text-[var(--blue)] border border-[var(--blue)]/30",
        purple:
          "bg-[var(--purple-subtle)] text-[var(--purple)] border border-[var(--purple)]/30",
        outline:
          "border border-[var(--border)] text-[var(--muted)] bg-transparent",
      },
      size: {
        sm: "px-1.5 py-0.2 rounded-[var(--radius-sm)] text-[10px] uppercase tracking-wider",
        default: "px-2 py-0.5 rounded-[var(--radius-sm)] text-[11px] uppercase tracking-wider",
        lg: "px-2.5 py-1 rounded-[var(--radius-sm)] text-xs uppercase tracking-wider",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  }
);

export interface BadgeProps
  extends React.HTMLAttributes<HTMLSpanElement>,
    VariantProps<typeof badgeVariants> {
  dot?: boolean;
  pulse?: boolean;
}

export function Badge({
  className,
  variant,
  size,
  dot,
  pulse,
  children,
  ...props
}: BadgeProps) {
  return (
    <span className={cn(badgeVariants({ variant, size, className }))} {...props}>
      {dot ? (
        <span
          className={cn(
            "h-1.5 w-1.5 rounded-full",
            variant === "success" && "bg-[var(--signal)]",
            variant === "danger" && "bg-[var(--danger)]",
            variant === "warning" && "bg-[var(--amber)]",
            variant === "info" && "bg-[var(--blue)]",
            variant === "purple" && "bg-[var(--purple)]",
            (!variant || variant === "default" || variant === "outline") && "bg-[var(--muted)]",
            pulse && "animate-pulse"
          )}
        />
      ) : null}
      {children}
    </span>
  );
}
