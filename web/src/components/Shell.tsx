import { useState } from "react";
import { useTranslation } from "react-i18next";
import { NavLink, Outlet, useLocation } from "react-router-dom";
import {
  AppWindow,
  BellRing,
  ChevronRight,
  DatabaseZap,
  Gauge,
  HardDrive,
  LogOut,
  Menu,
  Search,
  Settings,
  ShieldAlert,
  ShieldCheck,
  X,
  ScrollText,
} from "lucide-react";
import { motion, AnimatePresence } from "motion/react";
import { api } from "../lib/api";
import type { User } from "../App";
import { LocaleSelect, ThemeSelect } from "./PreferenceSelects";
import { Badge } from "./ui/Badge";
import { Button } from "./ui/Button";

const navigation = [
  {
    to: "/",
    label: "nav.overview",
    Icon: Gauge,
    colorClass: "icon-squircle-green",
    eyebrow: "overview.eyebrow",
    title: "overview.title",
  },
  {
    to: "/applications",
    label: "nav.applications",
    Icon: AppWindow,
    colorClass: "icon-squircle-indigo",
    eyebrow: "apps.eyebrow",
    title: "apps.title",
  },
  {
    to: "/devices",
    label: "settings.security",
    Icon: ShieldAlert,
    colorClass: "icon-squircle-amber",
    eyebrow: "settings.eyebrow",
    title: "settings.security",
  },
  {
    to: "/explorer",
    label: "nav.explorer",
    Icon: Search,
    colorClass: "icon-squircle-blue",
    eyebrow: "explorer.eyebrow",
    title: "explorer.title",
  },
  {
    to: "/logs",
    label: "nav.logs",
    Icon: ScrollText,
    colorClass: "icon-squircle-indigo",
    eyebrow: "logs.eyebrow",
    title: "logs.title",
  },
  {
    to: "/alerts",
    label: "nav.alerts",
    Icon: BellRing,
    colorClass: "icon-squircle-amber",
    eyebrow: "alerts.eyebrow",
    title: "alerts.title",
  },
  {
    to: "/migration",
    label: "nav.migration",
    Icon: DatabaseZap,
    colorClass: "icon-squircle-cyan",
    eyebrow: "migration.eyebrow",
    title: "migration.title",
  },
  {
    to: "/access",
    label: "nav.access",
    Icon: ShieldCheck,
    colorClass: "icon-squircle-purple",
    eyebrow: "access.eyebrow",
    title: "access.title",
  },
  {
    to: "/backup",
    label: "settings.backupRestore",
    Icon: HardDrive,
    colorClass: "icon-squircle-amber",
    eyebrow: "settings.eyebrow",
    title: "settings.backupRestore",
  },
  {
    to: "/settings",
    label: "nav.settings",
    Icon: Settings,
    colorClass: "icon-squircle-blue",
    eyebrow: "settings.eyebrow",
    title: "settings.title",
  },
] as const;

export function Shell({ user, onLogout }: { user: User; onLogout: () => void }) {
  const { t } = useTranslation();
  const location = useLocation();
  const [mobileOpen, setMobileOpen] = useState(false);

  const currentNav =
    navigation.find((item) =>
      item.to === "/"
        ? location.pathname === "/"
        : location.pathname.startsWith(item.to)
    ) ?? navigation[0];

  const logout = async () => {
    await api<void>("/api/v1/auth/logout", { method: "POST" }).catch(() => undefined);
    onLogout();
  };

  return (
    <div className="flex min-h-screen w-full bg-[var(--bg)] text-[var(--text)] antialiased selection:bg-[var(--signal)] selection:text-[var(--signal-ink)]">
      <a className="skip-link" href="#main-content">
        {t("shell.skipToContent")}
      </a>

      <aside
        className={`fixed inset-y-0 left-0 z-40 flex w-64 flex-col border-r border-[var(--border)] bg-[var(--bg-elevated)] p-3 transition-transform duration-300 ease-[cubic-bezier(0.22,1,0.36,1)] lg:translate-x-0 ${
          mobileOpen ? "translate-x-0 shadow-2xl" : "-translate-x-full"
        }`}
        aria-label="Primary navigation"
      >
        <div className="flex items-center justify-between px-2 py-3 border-b border-[var(--border-soft)] flex-shrink-0">
          <div className="brand-lockup">
            <div className="sonde-mark" aria-hidden="true">
              <span />
            </div>
            <div className="min-w-0">
              <strong>Sonde</strong>
              <small>Telemetry</small>
            </div>
          </div>

          <button
            type="button"
            onClick={() => setMobileOpen(false)}
            className="lg:hidden p-1.5 rounded-[var(--radius-sm)] text-[var(--muted)] hover:text-[var(--text)] cursor-pointer"
          >
            <X size={18} />
          </button>
        </div>

        <nav className="flex flex-col gap-0.5 my-3 flex-1 overflow-y-auto pr-1">
          {navigation.map((item) => {
            const isActive =
              item.to === "/"
                ? location.pathname === "/"
                : location.pathname.startsWith(item.to);
            const { Icon, colorClass } = item;

            return (
              <NavLink
                key={item.to}
                to={item.to}
                end={item.to === "/"}
                onClick={() => setMobileOpen(false)}
                className={`group relative flex items-center gap-3 px-2.5 py-2 rounded-[var(--radius-md)] text-xs font-semibold transition-colors duration-150 ${
                  isActive ? "text-[var(--text)]" : "text-[var(--muted)] hover:text-[var(--text)]"
                }`}
              >
                {isActive ? (
                  <motion.div
                    layoutId="sidebar-active-pill"
                    transition={{ type: "spring", stiffness: 420, damping: 34 }}
                    className="absolute inset-0 rounded-[var(--radius-md)] bg-[var(--signal-subtle)] border border-[var(--signal)]/25"
                  />
                ) : null}

                <span className={`relative z-10 p-1 rounded-[var(--radius-sm)] ${colorClass}`}>
                  <Icon size={15} />
                </span>

                <span className="relative z-10 flex-1 truncate">{t(item.label)}</span>
              </NavLink>
            );
          })}
        </nav>

        <div className="mt-auto pt-3 border-t border-[var(--border-soft)] flex-shrink-0">
          <div className="flex items-center gap-2.5 p-2 rounded-[var(--radius-md)] bg-[var(--input-bg)] border border-[var(--border-soft)]">
            <span className="flex h-8 w-8 items-center justify-center rounded-[var(--radius-sm)] bg-[var(--signal-subtle)] border border-[var(--signal)]/30 text-[var(--signal)] font-bold text-xs flex-shrink-0">
              {user.username.slice(0, 1).toUpperCase()}
            </span>
            <div className="flex-1 min-w-0">
              <strong className="block text-xs font-bold text-[var(--text)] truncate">
                {user.username}
              </strong>
              <small className="block text-[10px] text-[var(--muted)] truncate">
                {user.roles.includes("Super Admin") ? "Super Admin" : user.email}
              </small>
            </div>
            <Button
              variant="ghost"
              size="icon-sm"
              className="text-[var(--muted)] hover:text-[var(--danger)] hover:bg-[var(--danger-subtle)]"
              title={t("nav.logout")}
              aria-label={t("nav.logout")}
              onClick={logout}
              icon={<LogOut size={14} aria-hidden="true" />}
            />
          </div>
        </div>
      </aside>

      <AnimatePresence>
        {mobileOpen ? (
          <motion.button
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            className="fixed inset-0 z-30 bg-black/55 lg:hidden border-0 cursor-pointer"
            aria-label="Close navigation menu"
            onClick={() => setMobileOpen(false)}
          />
        ) : null}
      </AnimatePresence>

      <div className="flex flex-1 flex-col min-w-0 lg:ml-64">
        <header className="sticky top-0 z-20 flex h-14 w-full items-center justify-between border-b border-[var(--border)] bg-[var(--bg-elevated)] px-4 sm:px-6 lg:px-8 flex-shrink-0">
          <div className="flex items-center gap-3 min-w-0">
            <Button
              variant="secondary"
              size="icon"
              className="lg:hidden"
              aria-label={mobileOpen ? "Close menu" : "Open menu"}
              onClick={() => setMobileOpen((value) => !value)}
              icon={<Menu size={16} />}
            />

            <div className="flex items-center gap-2 min-w-0">
              <span className={`p-1 rounded-[var(--radius-sm)] ${currentNav.colorClass} hidden sm:inline-flex`}>
                <currentNav.Icon size={14} />
              </span>
              <span className="eyebrow hidden md:inline truncate">
                {t(currentNav.eyebrow)}
              </span>
              <ChevronRight size={13} className="text-[var(--faint)] hidden md:inline" />
              <h2 className="text-sm sm:text-base font-bold tracking-tight text-[var(--text)] truncate m-0">
                {t(currentNav.title)}
              </h2>
            </div>
          </div>

          <div className="flex items-center gap-2 sm:gap-3 flex-shrink-0">
            <Badge variant="success" dot pulse className="hidden sm:inline-flex py-1 px-2.5">
              {t("shell.systemLive")}
            </Badge>

            <ThemeSelect compact />
            <LocaleSelect compact />
          </div>
        </header>

        <main id="main-content" className="flex-1 w-full min-w-0">
          <motion.div
            key={location.pathname}
            initial={{ opacity: 0, y: 8 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -6 }}
            transition={{ duration: 0.22, ease: [0.22, 1, 0.36, 1] }}
            className="page-container"
          >
            <Outlet />
          </motion.div>
        </main>
      </div>
    </div>
  );
}
