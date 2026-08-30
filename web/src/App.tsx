import { lazy, Suspense, useEffect, useState } from "react";
import { BrowserRouter, Navigate, Route, Routes, useLocation, useNavigate } from "react-router-dom";
import { api, setCsrfToken } from "./lib/api";
import { Shell } from "./components/Shell";

const SetupPage = lazy(() => import("./pages/SetupPage").then((m) => ({ default: m.SetupPage })));
const LoginPage = lazy(() => import("./pages/LoginPage").then((m) => ({ default: m.LoginPage })));
const DashboardPage = lazy(() => import("./pages/DashboardPage").then((m) => ({ default: m.DashboardPage })));
const ApplicationsPage = lazy(() => import("./pages/ApplicationsPage").then((m) => ({ default: m.ApplicationsPage })));
const DevicesPage = lazy(() => import("./pages/DevicesPage").then((m) => ({ default: m.DevicesPage })));
const AlertsPage = lazy(() => import("./pages/AlertsPage").then((m) => ({ default: m.AlertsPage })));
const PlaceholderPage = lazy(() => import("./pages/PlaceholderPage").then((m) => ({ default: m.PlaceholderPage })));
const MigrationPage = lazy(() => import("./pages/MigrationPage").then((m) => ({ default: m.MigrationPage })));
const ExplorerPage = lazy(() => import("./pages/ExplorerPage").then((m) => ({ default: m.ExplorerPage })));
const AccessPage = lazy(() => import("./pages/AccessPage").then((m) => ({ default: m.AccessPage })));
const BackupPage = lazy(() => import("./pages/BackupPage").then((m) => ({ default: m.BackupPage })));
const SettingsPage = lazy(() => import("./pages/SettingsPage").then((m) => ({ default: m.SettingsPage })));
const PublicAppPage = lazy(() => import("./pages/PublicAppPage").then((m) => ({ default: m.PublicAppPage })));

export type User = {
  id: string;
  email: string;
  username: string;
  locale: string;
  roles: string[];
  csrfToken: string;
  totpEnabled?: boolean;
};

export function App() {
  return (
    <BrowserRouter>
      <AppRoutes />
    </BrowserRouter>
  );
}

function AppRoutes() {
  const [installed, setInstalled] = useState<boolean | null>(null);
  const [user, setUser] = useState<User | null>(null);
  const [loading, setLoading] = useState(true);
  const navigate = useNavigate();
  const location = useLocation();

  useEffect(() => {
    let active = true;
    void api<{ installed: boolean }>("/api/v1/setup/status")
      .then(async ({ installed: isInstalled }) => {
        if (!active) return;
        setInstalled(isInstalled);
        if (!isInstalled) {
          setLoading(false);
          return;
        }
        try {
          const current = await api<User>("/api/v1/auth/me");
          if (!active) return;
          setCsrfToken(current.csrfToken);
          setUser(current);
        } catch {
          if (active) setUser(null);
        } finally {
          if (active) setLoading(false);
        }
      })
      .catch(() => {
        if (active) {
          setInstalled(true);
          setUser(null);
          setLoading(false);
        }
      });
    return () => { active = false; };
  }, []);

  if (loading) {
    return <LoadingScreen />;
  }

  return (
    <Suspense fallback={<LoadingScreen />}>
      <Routes>
        <Route path="/p/:slug" element={<PublicAppPage />} />
        <Route
          path="/setup"
          element={
            installed ? (
              <Navigate to={user ? "/" : "/login"} replace />
            ) : (
              <SetupPage
                onComplete={() => {
                  setInstalled(true);
                  navigate("/login", { replace: true });
                }}
              />
            )
          }
        />
        <Route
          path="/login"
          element={
            !installed ? (
              <Navigate to="/setup" replace />
            ) : user ? (
              <Navigate to="/" replace />
            ) : (
              <LoginPage
                onLogin={(current) => {
                  setCsrfToken(current.csrfToken);
                  setUser(current);
                  navigate("/", { replace: true });
                }}
              />
            )
          }
        />
        <Route
          element={
            !installed ? (
              <Navigate to="/setup" replace />
            ) : !user ? (
              <Navigate to="/login" state={{ from: location }} replace />
            ) : (
              <Shell
                user={user}
                onLogout={() => {
                  setUser(null);
                  navigate("/login", { replace: true });
                }}
              />
            )
          }
        >
          <Route index element={<DashboardPage />} />
          <Route path="applications" element={<ApplicationsPage />} />
          <Route path="devices" element={<DevicesPage />} />
          <Route path="alerts" element={<AlertsPage />} />
          <Route path="explorer" element={<ExplorerPage />} />
          <Route path="migration" element={<MigrationPage />} />
          <Route path="access" element={<AccessPage />} />
          <Route path="backup" element={<BackupPage />} />
          <Route path="settings" element={<SettingsPage />} />
          <Route path="*" element={<Navigate to="/" replace />} />
        </Route>
      </Routes>
    </Suspense>
  );
}

function LoadingScreen() {
  return (
    <main className="boot-screen" aria-live="polite">
      <div className="sonde-mark" aria-hidden="true">
        <span />
      </div>
      <p>Calibrating Sonde</p>
    </main>
  );
}
