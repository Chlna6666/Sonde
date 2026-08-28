import { FormEvent, useEffect, useState } from "react";
import {
  AppWindow,
  Check,
  CheckCircle2,
  Copy,
  Eye,
  EyeOff,
  KeyRound,
  Lock,
  Plus,
  Settings,
  Shield,
  ShieldAlert,
  Trash2,
  UserCheck,
  UserPlus,
  Users,
  X,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { Modal } from "../components/Modal";
import { api } from "../lib/api";

type UserSummary = {
  id: string;
  email: string;
  username: string;
  locale: string;
  active: boolean;
  createdAt: number;
  roles: string[];
  assignedAppCount: number;
};

type Application = {
  id: string;
  name: string;
  slug: string;
};

export function AccessPage() {
  const { t } = useTranslation();
  const [users, setUsers] = useState<UserSummary[]>([]);
  const [applications, setApplications] = useState<Application[]>([]);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);

  // Modals state
  const [creatingUser, setCreatingUser] = useState(false);
  const [editingUser, setEditingUser] = useState<UserSummary | null>(null);
  const [resettingUser, setResettingUser] = useState<UserSummary | null>(null);
  const [assigningUser, setAssigningUser] = useState<UserSummary | null>(null);
  const [assignedAppIds, setAssignedAppIds] = useState<string[]>([]);
  const [currentUserId, setCurrentUserId] = useState<string>("");

  const loadData = () => {
    setLoading(true);
    api<{ id: string }>("/api/v1/auth/me")
      .then((me) => setCurrentUserId(me.id))
      .catch(() => {});

    Promise.all([
      api<UserSummary[]>("/api/v1/admin/users"),
      api<Application[]>("/api/v1/admin/applications"),
    ])
      .then(([userData, appData]) => {
        setUsers(userData);
        setApplications(appData);
        setLoading(false);
      })
      .catch((cause) => {
        setError(cause instanceof Error ? cause.message : String(cause));
        setLoading(false);
      });
  };

  useEffect(loadData, []);

  const openAppAssignment = async (user: UserSummary) => {
    try {
      const appIds = await api<string[]>(`/api/v1/admin/users/${user.id}/applications`);
      setAssignedAppIds(appIds);
      setAssigningUser(user);
    } catch {
      setAssignedAppIds([]);
      setAssigningUser(user);
    }
  };

  const handleSaveAppAssignments = async () => {
    if (!assigningUser) return;
    try {
      await api(`/api/v1/admin/users/${assigningUser.id}/applications`, {
        method: "PUT",
        body: JSON.stringify({
          applicationIds: assignedAppIds,
          role: "Manager",
        }),
      });
      setAssigningUser(null);
      loadData();
    } catch (cause) {
      alert(cause instanceof Error ? cause.message : t("common.error"));
    }
  };

  const handleDeleteUser = async (user: UserSummary) => {
    if (user.id === currentUserId) {
      alert(t("access.cannotDeleteSelf"));
      return;
    }
    if (!window.confirm(t("access.deleteConfirm"))) return;
    try {
      await api(`/api/v1/admin/users/${user.id}`, { method: "DELETE" });
      loadData();
    } catch (cause) {
      alert(cause instanceof Error ? cause.message : t("common.error"));
    }
  };

  return (
    <div className="page enter-page">
      <header className="page-header">
        <div>
          <span className="eyebrow">{t("access.title")}</span>
          <h1>{t("access.title")}</h1>
          <p>{t("access.subtitle")}</p>
        </div>
        <button className="primary-button compact" onClick={() => setCreatingUser(true)}>
          <UserPlus size={17} aria-hidden="true" />
          {t("access.new")}
        </button>
      </header>

      {error ? (
        <p className="form-error" role="alert">
          {error}
        </p>
      ) : null}

      <div className="glass-panel overflow-hidden">
        {loading ? (
          <div className="p-8 text-center text-xs text-[var(--muted)]">
            {t("common.loading")}
          </div>
        ) : users.length === 0 ? (
          <div className="empty-state">
            <Users aria-hidden="true" />
            <h2>{t("access.empty")}</h2>
          </div>
        ) : (
          <table className="w-full text-xs text-left border-collapse">
            <thead>
              <tr className="border-b border-[var(--border)] bg-[var(--panel-strong)]/60 text-[11px] font-bold text-[var(--muted)] uppercase tracking-wider">
                <th className="py-3 px-4">{t("access.username")}</th>
                <th className="py-3 px-4">{t("access.role")}</th>
                <th className="py-3 px-4">{t("access.assignedApps")}</th>
                <th className="py-3 px-4">{t("apps.status")}</th>
                <th className="py-3 px-4">{t("apps.createdAt")}</th>
                <th className="py-3 px-4 text-right">Actions</th>
              </tr>
            </thead>
            <tbody>
              {users.map((user) => {
                const isAdmin = user.roles.some(
                  (r) => r === "Super Admin" || r === "Admin"
                );
                return (
                  <tr key={user.id} className="border-b border-[var(--border-soft)] hover:bg-[var(--panel-hover)] transition-colors">
                    <td className="py-3 px-4">
                      <div className="flex items-center gap-3">
                        <div
                          className={`w-8 h-8 rounded-full border flex items-center justify-center text-xs font-bold flex-shrink-0 ${
                            isAdmin
                              ? "bg-[var(--signal-subtle)] border-[var(--signal)]/30 text-[var(--signal)]"
                              : "bg-[var(--panel-strong)] border-[var(--border)] text-[var(--text)]"
                          }`}
                        >
                          {user.username.slice(0, 2).toUpperCase()}
                        </div>
                        <div className="min-w-0">
                          <div className="flex items-center gap-1.5">
                            <strong className="text-xs font-bold text-[var(--text)] truncate">
                              {user.username}
                            </strong>
                            {user.id === currentUserId ? (
                              <span className="text-[10px] font-bold text-[var(--signal)]">
                                (You)
                              </span>
                            ) : null}
                          </div>
                          <span className="text-[11px] text-[var(--muted)] truncate block">
                            {user.email}
                          </span>
                        </div>
                      </div>
                    </td>
                    <td className="py-3 px-4">
                      {isAdmin ? (
                        <span className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-semibold bg-[var(--signal-subtle)] text-[var(--signal)] border border-[var(--signal)]/30">
                          <Shield size={13} className="flex-shrink-0" />
                          <span>{t("access.admin")}</span>
                        </span>
                      ) : (
                        <span className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-semibold bg-[var(--panel-strong)] text-[var(--muted)] border border-[var(--border)]">
                          <UserCheck size={13} className="flex-shrink-0" />
                          <span>{t("access.user")}</span>
                        </span>
                      )}
                    </td>
                    <td className="py-3 px-4">
                      {isAdmin ? (
                        <span className="text-xs font-bold text-[var(--signal)] font-sans">
                          {t("access.allApps")}
                        </span>
                      ) : (
                        <button
                          type="button"
                          className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-xl border border-[var(--border)] bg-[var(--input-bg)] text-xs font-semibold text-[var(--text)] hover:bg-[var(--panel-hover)] transition-colors cursor-pointer"
                          onClick={() => openAppAssignment(user)}
                        >
                          <AppWindow size={13} />
                          <span>{user.assignedAppCount} {t("nav.applications")}</span>
                        </button>
                      )}
                    </td>
                    <td className="py-3 px-4">
                      {user.active ? (
                        <span className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-semibold bg-[var(--signal-subtle)] text-[var(--signal)] border border-[var(--signal)]/30">
                          <CheckCircle2 size={13} className="flex-shrink-0" />
                          <span>{t("apps.active")}</span>
                        </span>
                      ) : (
                        <span className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-semibold bg-[var(--danger-subtle)] text-[var(--danger)] border border-[var(--danger)]/30">
                          <ShieldAlert size={13} className="flex-shrink-0" />
                          <span>Disabled</span>
                        </span>
                      )}
                    </td>
                    <td className="py-3 px-4 text-xs font-mono text-[var(--muted)] whitespace-nowrap">
                      {new Intl.DateTimeFormat(undefined, {
                        dateStyle: "medium",
                      }).format(user.createdAt)}
                    </td>
                    <td className="py-3 px-4 text-right">
                      <div className="flex items-center justify-end gap-1">
                        <button
                          type="button"
                          className="p-1.5 rounded-lg text-[var(--muted)] hover:text-[var(--text)] hover:bg-[var(--panel-hover)] transition-colors cursor-pointer"
                          title={t("access.manageApps")}
                          onClick={() => openAppAssignment(user)}
                        >
                          <AppWindow size={14} />
                        </button>
                        <button
                          type="button"
                          className="p-1.5 rounded-lg text-[var(--muted)] hover:text-[var(--text)] hover:bg-[var(--panel-hover)] transition-colors cursor-pointer"
                          title={t("access.resetPassword")}
                          onClick={() => setResettingUser(user)}
                        >
                          <KeyRound size={14} />
                        </button>
                        <button
                          type="button"
                          className="p-1.5 rounded-lg text-[var(--muted)] hover:text-[var(--text)] hover:bg-[var(--panel-hover)] transition-colors cursor-pointer"
                          title={t("access.editUser")}
                          onClick={() => setEditingUser(user)}
                        >
                          <Settings size={14} />
                        </button>
                        {user.id !== currentUserId ? (
                          <button
                            type="button"
                            className="p-1.5 rounded-lg text-[var(--muted)] hover:text-[var(--danger)] hover:bg-[var(--danger-subtle)] transition-colors cursor-pointer"
                            title={t("access.deleteUser")}
                            onClick={() => handleDeleteUser(user)}
                          >
                            <Trash2 size={14} />
                          </button>
                        ) : null}
                      </div>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </div>

      {/* Modal 1: Create Member Modal */}
      {creatingUser ? (
        <CreateUserModal
          applications={applications}
          onClose={() => setCreatingUser(false)}
          onSuccess={() => {
            setCreatingUser(false);
            loadData();
          }}
        />
      ) : null}

      {/* Modal 2: Edit Member Modal */}
      {editingUser ? (
        <EditUserModal
          user={editingUser}
          onClose={() => setEditingUser(null)}
          onSuccess={() => {
            setEditingUser(null);
            loadData();
          }}
        />
      ) : null}

      {/* Modal 3: Reset Password Modal */}
      {resettingUser ? (
        <ResetPasswordModal
          user={resettingUser}
          onClose={() => setResettingUser(null)}
          onSuccess={() => {
            setResettingUser(null);
            alert(t("access.passwordUpdated"));
          }}
        />
      ) : null}

      {/* Modal 4: Assign Applications Modal */}
      {assigningUser ? (
        <Modal
          size="md"
          title={`${t("access.manageApps")} · ${assigningUser.username}`}
          icon={<AppWindow size={20} />}
          onClose={() => setAssigningUser(null)}
          footer={
            <div className="flex justify-end gap-3 w-full">
              <button
                type="button"
                className="secondary-button compact"
                onClick={() => setAssigningUser(null)}
              >
                {t("common.cancel")}
              </button>
              <button className="primary-button compact" onClick={handleSaveAppAssignments}>
                {t("access.saveApps")}
              </button>
            </div>
          }
        >
          <p className="text-xs text-muted" style={{ margin: 0 }}>
            Select which applications <strong>{assigningUser.username}</strong> can view, manage API keys for, and inspect telemetry stats.
          </p>
          <div style={{ display: "flex", flexDirection: "column", gap: "8px", maxHeight: "300px", overflowY: "auto" }}>
            {applications.length === 0 ? (
              <p className="text-xs text-muted">No applications created yet.</p>
            ) : (
              applications.map((app) => {
                const isChecked = assignedAppIds.includes(app.id);
                return (
                  <label
                    key={app.id}
                    style={{
                      display: "flex",
                      alignItems: "center",
                      gap: "12px",
                      padding: "10px 14px",
                      background: "var(--input-bg)",
                      borderRadius: "6px",
                      border: isChecked
                        ? "1px solid color-mix(in srgb, var(--signal) 40%, var(--border))"
                        : "1px solid var(--border-soft)",
                      cursor: "pointer",
                    }}
                  >
                    <input
                      type="checkbox"
                      checked={isChecked}
                      onChange={(e) => {
                        if (e.target.checked) {
                          setAssignedAppIds([...assignedAppIds, app.id]);
                        } else {
                          setAssignedAppIds(assignedAppIds.filter((id) => id !== app.id));
                        }
                      }}
                    />
                    <div>
                      <strong style={{ fontSize: "0.82rem", display: "block" }}>{app.name}</strong>
                      <code style={{ fontSize: "0.7rem", color: "var(--muted)" }}>{app.slug}</code>
                    </div>
                  </label>
                );
              })
            )}
          </div>
        </Modal>
      ) : null}
    </div>
  );
}

function CreateUserModal({
  applications,
  onClose,
  onSuccess,
}: {
  applications: Application[];
  onClose: () => void;
  onSuccess: () => void;
}) {
  const { t } = useTranslation();
  const [showPassword, setShowPassword] = useState(false);
  const [selectedRole, setSelectedRole] = useState("User");
  const [selectedApps, setSelectedApps] = useState<string[]>([]);
  const [error, setError] = useState("");

  const handleSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setError("");
    const form = new FormData(event.currentTarget);
    try {
      const email = String(form.get("email"));
      const username = String(form.get("username"));
      const password = String(form.get("password"));
      const locale = String(form.get("locale") || "en");

      await api("/api/v1/admin/users", {
        method: "POST",
        body: JSON.stringify({
          email,
          username,
          password,
          locale,
          role: selectedRole,
          applicationIds: selectedRole === "Super Admin" ? [] : selectedApps,
        }),
      });
      onSuccess();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t("common.error"));
    }
  };

  return (
    <Modal
      size="md"
      title={t("access.new")}
      icon={<UserPlus size={20} />}
      onClose={onClose}
    >
      <form onSubmit={handleSubmit} style={{ display: "flex", flexDirection: "column", gap: "16px" }}>
        {error ? <p className="form-error">{error}</p> : null}

        <label className="field">
          <span>{t("access.username")}</span>
          <input name="username" placeholder="alice" required autoFocus />
        </label>

        <label className="field">
          <span>{t("access.email")}</span>
          <input name="email" type="email" placeholder="alice@example.com" required />
        </label>

        <label className="field">
          <span>{t("access.initialPassword")}</span>
          <div style={{ position: "relative" }}>
            <input
              name="password"
              type={showPassword ? "text" : "password"}
              minLength={8}
              placeholder="Min 8 characters"
              required
            />
            <button
              type="button"
              tabIndex={-1}
              onClick={() => setShowPassword(!showPassword)}
              style={{
                position: "absolute",
                right: "10px",
                top: "50%",
                transform: "translateY(-50%)",
                background: "none",
                border: "none",
                color: "var(--muted)",
                cursor: "pointer",
              }}
            >
              {showPassword ? <EyeOff size={15} /> : <Eye size={15} />}
            </button>
          </div>
        </label>

        <label className="field">
          <span>{t("access.role")}</span>
          <select
            value={selectedRole}
            onChange={(e) => setSelectedRole(e.target.value)}
          >
            <option value="User">{t("access.user")}</option>
            <option value="Super Admin">{t("access.admin")}</option>
          </select>
        </label>

        {selectedRole === "User" && applications.length > 0 ? (
          <div>
            <span className="text-xs font-semibold text-muted uppercase block mb-2">
              {t("access.manageApps")}
            </span>
            <div style={{ display: "flex", flexDirection: "column", gap: "6px", maxHeight: "150px", overflowY: "auto" }}>
              {applications.map((app) => (
                <label
                  key={app.id}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: "10px",
                    padding: "6px 10px",
                    background: "var(--input-bg)",
                    borderRadius: "6px",
                    fontSize: "0.78rem",
                    cursor: "pointer",
                  }}
                >
                  <input
                    type="checkbox"
                    checked={selectedApps.includes(app.id)}
                    onChange={(e) => {
                      if (e.target.checked) {
                        setSelectedApps([...selectedApps, app.id]);
                      } else {
                        setSelectedApps(selectedApps.filter((id) => id !== app.id));
                      }
                    }}
                  />
                  <span>{app.name} ({app.slug})</span>
                </label>
              ))}
            </div>
          </div>
        ) : null}

        <div style={{ display: "flex", justifyContent: "flex-end", gap: "10px", marginTop: "8px" }}>
          <button type="button" className="secondary-button compact" onClick={onClose}>
            {t("common.cancel")}
          </button>
          <button className="primary-button compact">{t("common.create")}</button>
        </div>
      </form>
    </Modal>
  );
}

function EditUserModal({
  user,
  onClose,
  onSuccess,
}: {
  user: UserSummary;
  onClose: () => void;
  onSuccess: () => void;
}) {
  const { t } = useTranslation();
  const isAdmin = user.roles.some((r) => r === "Super Admin" || r === "Admin");
  const [selectedRole, setSelectedRole] = useState(isAdmin ? "Super Admin" : "User");
  const [active, setActive] = useState(user.active);
  const [error, setError] = useState("");

  const handleSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setError("");
    const form = new FormData(event.currentTarget);
    try {
      const email = String(form.get("email"));
      const username = String(form.get("username"));

      await api(`/api/v1/admin/users/${user.id}`, {
        method: "PATCH",
        body: JSON.stringify({
          email,
          username,
          role: selectedRole,
          active,
        }),
      });
      onSuccess();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t("common.error"));
    }
  };

  return (
    <Modal
      size="sm"
      title={`${t("access.editUser")} · ${user.username}`}
      icon={<Settings size={20} />}
      onClose={onClose}
    >
      <form onSubmit={handleSubmit} style={{ display: "flex", flexDirection: "column", gap: "16px" }}>
        {error ? <p className="form-error">{error}</p> : null}

        <label className="field">
          <span>{t("access.username")}</span>
          <input name="username" defaultValue={user.username} required />
        </label>

        <label className="field">
          <span>{t("access.email")}</span>
          <input name="email" type="email" defaultValue={user.email} required />
        </label>

        <label className="field">
          <span>{t("access.role")}</span>
          <select
            value={selectedRole}
            onChange={(e) => setSelectedRole(e.target.value)}
          >
            <option value="User">{t("access.user")}</option>
            <option value="Super Admin">{t("access.admin")}</option>
          </select>
        </label>

        <label style={{ display: "flex", alignItems: "center", gap: "10px", cursor: "pointer", fontSize: "0.78rem" }}>
          <input
            type="checkbox"
            checked={active}
            onChange={(e) => setActive(e.target.checked)}
          />
          <span>{t("access.accountActive")}</span>
        </label>

        <div style={{ display: "flex", justifyContent: "flex-end", gap: "10px", marginTop: "8px" }}>
          <button type="button" className="secondary-button compact" onClick={onClose}>
            {t("common.cancel")}
          </button>
          <button className="primary-button compact">{t("common.save")}</button>
        </div>
      </form>
    </Modal>
  );
}

function ResetPasswordModal({
  user,
  onClose,
  onSuccess,
}: {
  user: UserSummary;
  onClose: () => void;
  onSuccess: () => void;
}) {
  const { t } = useTranslation();
  const [showPassword, setShowPassword] = useState(false);
  const [error, setError] = useState("");

  const handleSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setError("");
    const form = new FormData(event.currentTarget);
    try {
      const newPassword = String(form.get("newPassword"));
      await api(`/api/v1/admin/users/${user.id}/password`, {
        method: "POST",
        body: JSON.stringify({ newPassword }),
      });
      onSuccess();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t("common.error"));
    }
  };

  return (
    <Modal
      size="sm"
      title={`${t("access.resetPassword")} · ${user.username}`}
      icon={<KeyRound size={20} />}
      onClose={onClose}
    >
      <form onSubmit={handleSubmit} style={{ display: "flex", flexDirection: "column", gap: "16px" }}>
        {error ? <p className="form-error">{error}</p> : null}

        <label className="field">
          <span>{t("access.newPassword")}</span>
          <div style={{ position: "relative" }}>
            <input
              name="newPassword"
              type={showPassword ? "text" : "password"}
              minLength={8}
              placeholder="Min 8 characters"
              required
              autoFocus
            />
            <button
              type="button"
              tabIndex={-1}
              onClick={() => setShowPassword(!showPassword)}
              style={{
                position: "absolute",
                right: "10px",
                top: "50%",
                transform: "translateY(-50%)",
                background: "none",
                border: "none",
                color: "var(--muted)",
                cursor: "pointer",
              }}
            >
              {showPassword ? <EyeOff size={15} /> : <Eye size={15} />}
            </button>
          </div>
        </label>

        <div style={{ display: "flex", justifyContent: "flex-end", gap: "10px", marginTop: "8px" }}>
          <button type="button" className="secondary-button compact" onClick={onClose}>
            {t("common.cancel")}
          </button>
          <button className="primary-button compact">{t("common.save")}</button>
        </div>
      </form>
    </Modal>
  );
}
