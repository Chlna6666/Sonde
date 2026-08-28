import { Languages, SunMoon } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { getThemeMode, setThemeMode, type ThemeMode } from "../lib/theme";
import type { Locale } from "../i18n";
import { CustomSelect } from "./CustomSelect";

export function LocaleSelect({ compact = false }: { compact?: boolean }) {
  const { t, i18n } = useTranslation();
  const locale: Locale = i18n.language.toLowerCase().startsWith("zh") ? "zh-CN" : "en";
  const label = t("preferences.language");

  return (
    <div className="flex items-center gap-2">
      {!compact ? <span className="text-xs text-[var(--muted)]">{label}</span> : null}
      <CustomSelect
        label={label}
        value={locale}
        options={[
          { value: "en", label: "English" },
          { value: "zh-CN", label: "简体中文" },
        ]}
        onChange={(value) => void i18n.changeLanguage(value)}
        icon={<Languages size={13} aria-hidden="true" />}
        compact={compact}
      />
    </div>
  );
}

export function ThemeSelect({ compact = false }: { compact?: boolean }) {
  const { t } = useTranslation();
  const [mode, setMode] = useState<ThemeMode>(() => getThemeMode());
  const label = t("preferences.theme");

  return (
    <div className="flex items-center gap-2">
      {!compact ? <span className="text-xs text-[var(--muted)]">{label}</span> : null}
      <CustomSelect
        label={label}
        value={mode}
        options={[
          { value: "auto", label: t("preferences.auto") },
          { value: "light", label: t("preferences.light") },
          { value: "dark", label: t("preferences.dark") },
        ]}
        onChange={(value: ThemeMode) => {
          setMode(value);
          setThemeMode(value);
        }}
        icon={<SunMoon size={13} aria-hidden="true" />}
        compact={compact}
      />
    </div>
  );
}
