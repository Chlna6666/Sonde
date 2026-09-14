import i18n from "i18next";
import { initReactI18next } from "react-i18next";

export type Locale = "en" | "zh-CN";
export const defaultLng: Locale = "zh-CN";
const LOCALE_KEY = "sonde_locale";

export function getInitialLocale(): Locale {
  try {
    const saved = localStorage.getItem(LOCALE_KEY);
    if (saved === "en" || saved === "zh-CN") return saved;
    const nav = typeof navigator !== "undefined" ? navigator.language.toLowerCase() : "";
    if (nav.startsWith("zh")) return "zh-CN";
    if (nav.startsWith("en")) return "en";
  } catch {
    // ignore
  }
  return defaultLng;
}

export async function loadLocaleResource(lng: string): Promise<Record<string, string>> {
  const normalized = lng.toLowerCase().startsWith("zh") ? "zh-CN" : "en";
  if (normalized === "zh-CN") {
    const mod = await import("./locales/zh-CN.json");
    return mod.default;
  }
  const mod = await import("./locales/en.json");
  return mod.default;
}

const dynamicLoaderBackend = {
  type: "backend" as const,
  init: () => {},
  read(
    language: string,
    _namespace: string,
    callback: (err: Error | null, data?: Record<string, string>) => void
  ) {
    loadLocaleResource(language)
      .then((data) => callback(null, data))
      .catch((err) => callback(err instanceof Error ? err : new Error(String(err))));
  },
};

export const i18nPromise = i18n
  .use(dynamicLoaderBackend)
  .use(initReactI18next)
  .init({
    lng: getInitialLocale(),
    load: "currentOnly",
    fallbackLng: false,
    interpolation: {
      escapeValue: false,
    },
    react: {
      useSuspense: false,
    },
  });

i18n.on("languageChanged", (lng) => {
  try {
    const normalized = lng.toLowerCase().startsWith("zh") ? "zh-CN" : "en";
    localStorage.setItem(LOCALE_KEY, normalized);
  } catch {
    // ignore
  }
});

export async function initI18n(): Promise<typeof i18n> {
  await i18nPromise;
  return i18n;
}

export default i18n;
