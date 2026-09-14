import React, { forwardRef } from "react";
import { cva, type VariantProps } from "class-variance-authority";
import { Loader2 } from "lucide-react";
import { cn } from "../../lib/utils";

export const buttonVariants = cva(
  "inline-flex items-center justify-center gap-1.5 whitespace-nowrap text-xs font-semibold transition-all duration-150 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus)] disabled:pointer-events-none disabled:opacity-50 cursor-pointer select-none",
  {
    variants: {
      variant: {
        default:
          "bg-[var(--signal)] text-[var(--signal-ink)] hover:bg-[var(--primary-hover)] active:translate-y-px",
        secondary:
          "bg-[var(--input-bg)] text-[var(--text)] border border-[var(--border)] hover:border-[var(--border-highlight)] hover:bg-[var(--panel-hover)] active:translate-y-px",
        outline:
          "border border-[var(--border)] bg-transparent text-[var(--text)] hover:bg-[var(--input-bg)] active:translate-y-px",
        ghost:
          "bg-transparent text-[var(--muted)] hover:text-[var(--text)] hover:bg-[var(--input-bg)] active:translate-y-px",
        danger:
          "bg-[var(--danger)] text-white hover:bg-[var(--danger-strong)] active:translate-y-px",
        "danger-outline":
          "border border-[var(--danger)]/30 text-[var(--danger)] bg-transparent hover:bg-[var(--danger-subtle)] active:translate-y-px",
      },
      size: {
        default: "h-9 px-3.5 rounded-[var(--radius-md)]",
        sm: "h-8 px-2.5 text-xs rounded-[var(--radius-md)]",
        lg: "h-10 px-4 text-sm rounded-[var(--radius-lg)]",
        icon: "h-8 w-8 p-0 rounded-[var(--radius-md)]",
        "icon-sm": "h-7 w-7 p-0 rounded-[var(--radius-sm)]",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  }
);

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {
  loading?: boolean;
  icon?: React.ReactNode;
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, loading, icon, children, disabled, type = "button", ...props }, ref) => {
    return (
      <button
        ref={ref}
        type={type}
        className={cn(buttonVariants({ variant, size, className }))}
        disabled={disabled || loading}
        {...props}
      >
        {loading ? (
          <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
        ) : icon ? (
          <span className="flex-shrink-0">{icon}</span>
        ) : null}
        {children}
      </button>
    );
  }
);

Button.displayName = "Button";
