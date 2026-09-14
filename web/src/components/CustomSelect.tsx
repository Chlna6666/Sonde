import { Check, ChevronDown } from "lucide-react";
import { useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { motion, AnimatePresence } from "motion/react";
import "../styles/preferences.css";

export type SelectOption<T extends string> = {
  value: T;
  label: string;
};

type CustomSelectProps<T extends string> = {
  label: string;
  value: T;
  options: readonly SelectOption<T>[];
  onChange: (value: T) => void;
  icon?: ReactNode;
  compact?: boolean;
};

export function CustomSelect<T extends string>({
  label,
  value,
  options,
  onChange,
  icon,
  compact = false,
}: CustomSelectProps<T>) {
  const id = useId();
  const rootRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const selectedIndex = Math.max(0, options.findIndex((option) => option.value === value));
  const [activeIndex, setActiveIndex] = useState(selectedIndex);
  const [open, setOpen] = useState(false);
  const selectedLabel = options[selectedIndex]?.label ?? value;

  useEffect(() => {
    function closeOnOutsidePress(event: PointerEvent) {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    }
    document.addEventListener("pointerdown", closeOnOutsidePress);
    return () => document.removeEventListener("pointerdown", closeOnOutsidePress);
  }, []);

  function openMenu() {
    setActiveIndex(selectedIndex);
    setOpen(true);
  }

  function choose(index: number) {
    const option = options[index];
    if (!option) return;
    onChange(option.value);
    setOpen(false);
    buttonRef.current?.focus();
  }

  function handleKeyDown(event: KeyboardEvent<HTMLButtonElement>) {
    const lastIndex = options.length - 1;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (!open) return openMenu();
      const direction = event.key === "ArrowDown" ? 1 : -1;
      setActiveIndex((index) => Math.min(lastIndex, Math.max(0, index + direction)));
      return;
    }
    if (open && event.key === "Home") {
      event.preventDefault();
      setActiveIndex(0);
    } else if (open && event.key === "End") {
      event.preventDefault();
      setActiveIndex(lastIndex);
    } else if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      open ? choose(activeIndex) : openMenu();
    } else if (event.key === "Escape") {
      event.preventDefault();
      setOpen(false);
    } else if (event.key === "Tab") {
      setOpen(false);
    }
  }

  return (
    <div className="relative inline-block text-left" ref={rootRef}>
      <button
        ref={buttonRef}
        type="button"
        role="combobox"
        aria-label={label}
        aria-expanded={open}
        aria-haspopup="listbox"
        aria-controls={`${id}-listbox`}
        aria-activedescendant={open ? `${id}-option-${activeIndex}` : undefined}
        onClick={() => (open ? setOpen(false) : openMenu())}
        onKeyDown={handleKeyDown}
        className={`inline-flex items-center justify-between gap-1.5 rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--input-bg)] font-semibold text-[var(--text)] hover:bg-[var(--panel-hover)] hover:border-[var(--border-highlight)] active:translate-y-px transition-colors cursor-pointer ${
          compact ? "h-8 px-2.5 text-[11px]" : "h-9 px-3 text-xs"
        }`}
      >
        {icon ? <span className="text-[var(--muted)] flex-shrink-0">{icon}</span> : null}
        <span className="truncate">{selectedLabel}</span>
        <ChevronDown
          aria-hidden="true"
          size={12}
          className={`text-[var(--muted)] transition-transform duration-200 ${open ? "rotate-180" : ""}`}
        />
      </button>

      <AnimatePresence>
        {open ? (
          <motion.div
            initial={{ opacity: 0, scale: 0.95, y: -4 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.95, y: -4 }}
            transition={{ type: "spring", stiffness: 450, damping: 30 }}
            className="absolute right-0 top-full mt-1.5 z-50 min-w-[140px] overflow-hidden rounded-[var(--radius-lg)] border border-[var(--border)] bg-[var(--bg-float)] p-1 shadow-xl"
            id={`${id}-listbox`}
            role="listbox"
            aria-label={label}
          >
            {options.map((option, index) => {
              const isSelected = option.value === value;
              return (
                <div
                  id={`${id}-option-${index}`}
                  key={option.value}
                  role="option"
                  aria-selected={isSelected}
                  className={`flex items-center justify-between gap-3 px-3 py-2 rounded-[var(--radius-sm)] text-xs font-semibold cursor-pointer transition-colors ${
                    isSelected
                      ? "bg-[var(--signal-subtle)] text-[var(--signal)] font-bold"
                      : "text-[var(--text)] hover:bg-[var(--panel-hover)]"
                  }`}
                  onMouseEnter={() => setActiveIndex(index)}
                  onMouseDown={(event) => event.preventDefault()}
                  onClick={() => choose(index)}
                >
                  <span className="truncate">{option.label}</span>
                  {isSelected ? <Check aria-hidden="true" size={14} className="text-[var(--signal)] flex-shrink-0" /> : null}
                </div>
              );
            })}
          </motion.div>
        ) : null}
      </AnimatePresence>
    </div>
  );
}
