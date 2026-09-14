import React, { forwardRef } from "react";
import { cn } from "../../lib/utils";

export interface InputProps extends React.InputHTMLAttributes<HTMLInputElement> {
  error?: boolean;
}

export const Input = forwardRef<HTMLInputElement, InputProps>(
  ({ className, type = "text", error, ...props }, ref) => {
    return (
      <input
        type={type}
        ref={ref}
        className={cn(
          "flex h-9 w-full rounded-[var(--radius-md)] border bg-[var(--input-bg)] px-3 py-1.5 text-xs text-[var(--text)] transition-colors placeholder:text-[var(--faint)] focus:outline-none focus:ring-2 focus:ring-[var(--focus)] disabled:cursor-not-allowed disabled:opacity-50",
          error
            ? "border-[var(--danger)] focus:ring-[var(--danger-glow)]"
            : "border-[var(--border)] focus:border-[var(--border-highlight)]",
          className
        )}
        {...props}
      />
    );
  }
);

Input.displayName = "Input";
