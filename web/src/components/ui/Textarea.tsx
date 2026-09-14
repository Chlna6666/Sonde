import React, { forwardRef } from "react";
import { cn } from "../../lib/utils";

export interface TextareaProps extends React.TextareaHTMLAttributes<HTMLTextAreaElement> {
  error?: boolean;
}

export const Textarea = forwardRef<HTMLTextAreaElement, TextareaProps>(
  ({ className, error, ...props }, ref) => {
    return (
      <textarea
        ref={ref}
        className={cn(
          "flex min-h-[80px] w-full rounded-[var(--radius-md)] border bg-[var(--input-bg)] px-3 py-2 text-xs text-[var(--text)] transition-colors placeholder:text-[var(--faint)] focus:outline-none focus:ring-2 focus:ring-[var(--focus)] disabled:cursor-not-allowed disabled:opacity-50",
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

Textarea.displayName = "Textarea";
