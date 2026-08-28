import { useState } from "react";
import { useTranslation } from "react-i18next";
import { NavLink, Outlet, useLocation } from "react-router-dom";
import {
  Activity,
  AppWindow,
  BellRing,
  ChevronRight,
  DatabaseZap,
  Gauge,
  LogOut,
  Menu,
  Radio,
  Search,
  Settings,
  ShieldCheck,
  X,
  RadioTower,
} from "lucide-react";
import { motion, AnimatePresence } from "motion/react";
import { api } from "../lib/api";
import type { User } from "../App";
import { LocaleSelect, ThemeSelect } from "./PreferenceSelects";

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
    to: "/explorer",
    label: "nav.explorer",
    Icon: Search,
    colorClass: "icon-squircle-blue",
    eyebrow: "explorer.eyebrow",
    title: "explorer.title",
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
    <div className="flex min-h-screen w-full bg-[var(--bg)] text-[var(--text)] font-sans antialiased selection:bg-[var(--signal)] selection:text-white">
      <a className="skip-link" href="#main-content">
        Skip to main content
      </a>

      {/* Desktop & Tablet Sidebar (Fixed 260px width with glassmorphism) */}
      <aside
        className={`fixed inset-y-0 left-0 z-40 flex w-64 flex-col border-r border-[var(--border-soft)] bg-[var(--bg-elevated)] p-4 backdrop-blur-2xl transition-transform duration-300 ease-[cubic-bezier(0.16,1,0.3,1)] lg:translate-x-0 ${
          mobileOpen ? "translate-x-0 shadow-2xl" : "-translate-x-full"
        }`}
        aria-label="Primary navigation"
      >
        {/* Brand Lockup Header */}
        <div className="flex items-center justify-between px-2 py-3 border-b border-[var(--border-soft)] flex-shrink-0">
          <div className="flex items-center gap-3">
            <div className="relative flex h-9 w-9 items-center justify-center rounded-xl bg-[var(--signal)] text-white shadow-md shadow-[var(--signal)]/25 flex-shrink-0">
              <RadioTower size={18} />
              <span className="absolute -top-0.5 -right-0.5 h-2.5 w-2.5 rounded-full bg-white border-2 border-[var(--signal)] animate-ping" />
            </div>
            <div className="min-w-0">
              <strong className="block text-base font-extrabold tracking-tight text-[var(--text)] leading-none">
                Sonde
              </strong>
              <small className="block text-[11px] font-medium text-[var(--muted)] truncate mt-1">
                {t("brand.name")} Telemetry
              </small>
            </div>
          </div>

          <button
            type="button"
            onClick={() => setMobileOpen(false)}
            className="lg:hidden p-1.5 rounded-lg text-[var(--muted)] hover:text-[var(--text)] cursor-pointer"
          >
            <X size={18} />
          </button>
        </div>

        {/* Navigation links with Sliding Pill Active Indicator */}
        <nav className="flex flex-col gap-1 my-4 flex-1 overflow-y-auto pr-1">
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
                className={`group relative flex items-center gap-3 px-3 py-2.5 rounded-xl text-xs font-semibold transition-colors duration-150 ${
                  isActive ? "text-[var(--text)] font-bold" : "text-[var(--muted)] hover:text-[var(--text)]"
                }`}
              >
                {/* Active Sliding Pill Animation */}
                {isActive ? (
                  <motion.div
                    layoutId="sidebar-active-pill"
                    transition={{ type: "spring", stiffness: 380, damping: 30 }}
                    className="absolute inset-0 rounded-xl bg-[var(--panel-strong)] border border-[var(--border)] shadow-sm"
                  />
                ) : null}

                <span className={`relative z-10 p-1.5 rounded-lg ${colorClass}`}>
                  <Icon size={16} />
                </span>

                <span className="relative z-10 flex-1 truncate">{t(item.label)}</span>

                <ChevronRight
                  size={14}
                  className={`relative z-10 transition-transform duration-200 ${
                    isActive ? "opacity-90 translate-x-0.5 text-[var(--signal)]" : "opacity-0 group-hover:opacity-40"
                  }`}
                />
              </NavLink>
            );
          })}
        </nav>

        {/* User Card with Inline Logout */}
        <div className="mt-auto pt-3 border-t border-[var(--border-soft)] flex-shrink-0">
          <div className="flex items-center gap-2.5 p-2 rounded-2xl bg-[var(--input-bg)] border border-[var(--border-soft)] hover:border-[var(--border)] transition-colors">
            <span className="flex h-8 w-8 items-center justify-center rounded-xl bg-[var(--signal-subtle)] border border-[var(--signal)]/30 text-[var(--signal)] font-bold text-xs flex-shrink-0">
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
            <button
              type="button"
              className="flex h-8 w-8 items-center justify-center rounded-xl text-[var(--muted)] hover:text-[var(--danger)] hover:bg-[var(--danger-subtle)] active:scale-95 transition-all flex-shrink-0 cursor-pointer"
              title={t("nav.logout")}
              aria-label={t("nav.logout")}
              onClick={logout}
            >
              <LogOut size={15} aria-hidden="true" />
            </button>
          </div>
        </div>
      </aside>

      {/* Mobile Drawer Overlay */}
      <AnimatePresence>
        {mobileOpen ? (
          <motion.button
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            className="fixed inset-0 z-30 bg-black/60 backdrop-blur-md lg:hidden border-0 cursor-pointer"
            aria-label="Close navigation menu"
            onClick={() => setMobileOpen(false)}
          />
        ) : null}
      </AnimatePresence>

      {/* Main Workspace Area */}
      <div className="flex flex-1 flex-col min-w-0 lg:ml-64">
        {/* Apple Translucent Topbar */}
        <header className="sticky top-0 z-20 flex h-16 w-full items-center justify-between border-b border-[var(--border-soft)] bg-[var(--bg-elevated)] px-4 sm:px-6 lg:px-8 backdrop-blur-2xl flex-shrink-0">
          {/* Left Side: Mobile Menu Button & Breadcrumb */}
          <div className="flex items-center gap-3 min-w-0">
            <button
              type="button"
              className="flex h-9 w-9 items-center justify-center rounded-xl border border-[var(--border)] bg-[var(--panel)] text-[var(--text)] lg:hidden cursor-pointer active:scale-95 transition-all flex-shrink-0"
              aria-label={mobileOpen ? "Close menu" : "Open menu"}
              onClick={() => setMobileOpen((value) => !value)}
            >
              <Menu size={18} />
            </button>

            <div className="flex items-center gap-2 min-w-0">
              <span className={`p-1 rounded-lg ${currentNav.colorClass} hidden sm:inline-flex`}>
                <currentNav.Icon size={14} />
              </span>
              <span className="text-xs font-semibold text-[var(--muted)] hidden md:inline truncate">
                {t(currentNav.eyebrow)}
              </span>
              <ChevronRight size={13} className="text-[var(--faint)] hidden md:inline" />
              <h2 className="text-sm sm:text-base font-extrabold tracking-tight text-[var(--text)] truncate m-0">
                {t(currentNav.title)}
              </h2>
            </div>
          </div>

          {/* Right Side: Live Ingestion Pulse + Controls */}
          <div className="flex items-center gap-2 sm:gap-3 flex-shrink-0">
            <div className="hidden sm:flex items-center gap-2 px-3 py-1.5 rounded-full bg-[var(--signal-subtle)] border border-[var(--signal)]/30 text-xs font-medium text-[var(--signal)]">
              <span className="relative flex h-2 w-2">
                <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-[var(--signal)] opacity-75" />
                <span className="relative inline-flex rounded-full h-2 w-2 bg-[var(--signal)]" />
              </span>
              <span className="font-semibold text-[11px]">System Live</span>
            </div>

            <ThemeSelect compact />
            <LocaleSelect compact />
          </div>
        </header>

        {/* Content Viewport with Spring Page Transition */}
        <main id="main-content" className="flex-1 w-full min-w-0">
          <motion.div
            key={location.pathname}
            initial={{ opacity: 0, y: 6 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -6 }}
            transition={{ duration: 0.22, ease: [0.16, 1, 0.3, 1] }}
            className="page-container"
          >
            <Outlet />
          </motion.div>
        </main>
      </div>
    </div>
  );
}
