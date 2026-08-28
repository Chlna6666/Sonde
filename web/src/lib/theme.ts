export type ThemeMode = "auto" | "light" | "dark";

const QUERY = "(prefers-color-scheme: dark)";
const STORAGE_KEY = "sonde-theme";

export function getThemeMode(): ThemeMode {
  const stored = localStorage.getItem(STORAGE_KEY);
  return stored === "light" || stored === "dark" || stored === "auto" ? stored : "auto";
}

export function setThemeMode(mode: ThemeMode) {
  localStorage.setItem(STORAGE_KEY, mode);
  applyTheme(mode);
}

export function initializeTheme() {
  const media = matchMedia(QUERY);
  applyTheme(getThemeMode());
  media.addEventListener("change", () => {
    if (getThemeMode() === "auto") applyTheme("auto");
  });
}

function applyTheme(mode: ThemeMode) {
  const resolved = mode === "auto" ? (matchMedia(QUERY).matches ? "dark" : "light") : mode;
  document.documentElement.dataset.theme = resolved;
  document.documentElement.dataset.themeMode = mode;
  document.querySelector('meta[name="theme-color"]')?.setAttribute("content", resolved === "dark" ? "#090e12" : "#edf2f1");
}
