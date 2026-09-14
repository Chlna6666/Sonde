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
import { Button, Card, Badge, Table, TableHeader, TableBody, TableRow, TableHead, TableCell, Input, EmptyState } from "../components/ui";
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
  const [currentUserRoles, setCurrentUserRoles] = useState<string[]>([]);

  const loadData = () => {
    setLoading(true);
    api<{ id: string; roles: string[] }>("/api/v1/auth/me")
      .then((me) => {
        setCurrentUserId(me.id);
        setCurrentUserRoles(me.roles);
      })
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
        <Button size="sm" onClick={() => setCreatingUser(true)} icon={<UserPlus size={15} aria-hidden="true" />}>
          {t("access.new")}
        </Button>
      </header>

      {error ? (
        <p className="text-xs text-[var(--danger)] mb-4 font-medium" role="alert">
          {error}
        </p>
      ) : null}

      <div>
        {loading ? (
          <div className="p-8 text-center text-xs text-[var(--muted)]">
            {t("common.loading")}
          </div>
        ) : users.length === 0 ? (
          <EmptyState
            icon={<Users size={28} />}
            title={t("access.empty")}
            action={<Button size="sm" onClick={() => setCreatingUser(true)} icon={<UserPlus size={14} />}>{t("access.new")}</Button>}
          />
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>{t("access.username")}</TableHead>
                <TableHead>{t("access.role")}</TableHead>
                <TableHead>{t("access.assignedApps")}</TableHead>
                <TableHead>{t("apps.status")}</TableHead>
                <TableHead>{t("apps.createdAt")}</TableHead>
                <TableHead className="text-right">{t("common.actions")}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {users.map((user) => {
                const isAdmin = user.roles.some(
                  (r) => r === "Super Admin" || r === "Admin"
                );
                return (
                  <TableRow key={user.id}>
                    <TableCell>
                      <div className="flex items-center gap-3">
                        <div
                          className={`w-8 h-8 rounded-[var(--radius-sm)] border flex items-center justify-center text-xs font-bold flex-shrink-0 ${
                            isAdmin
                              ? "bg-[var(--signal-subtle)] border-[var(--signal)]/30 text-[var(--signal)]"
                              : "bg-[var(--input-bg)] border-[var(--border)] text-[var(--text)]"
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
                              <Badge variant="success" size="sm">
                                {t("access.you")}
                              </Badge>
                            ) : null}
                          </div>
                          <span className="text-[11px] text-[var(--muted)] truncate block">
                            {user.email}
                          </span>
                        </div>
                      </div>
                    </TableCell>
                    <TableCell>
                      {isAdmin ? (
                        <Badge variant="success" size="sm">
                          <Shield size={12} className="flex-shrink-0" />
                          <span>{t("access.admin")}</span>
                        </Badge>
                      ) : (
                        <Badge variant="outline" size="sm">
                          <UserCheck size={12} className="flex-shrink-0" />
                          <span>{t("access.user")}</span>
                        </Badge>
                      )}
                    </TableCell>
                    <TableCell>
                      {isAdmin ? (
                        <span className="text-xs font-bold text-[var(--signal)] font-sans">
                          {t("access.allApps")}
                        </span>
                      ) : (
                        <Button
                          variant="secondary"
                          size="sm"
                          onClick={() => openAppAssignment(user)}
                          icon={<AppWindow size={13} />}
                        >
                          <span>{user.assignedAppCount} {t("nav.applications")}</span>
                        </Button>
                      )}
                    </TableCell>
                    <TableCell>
                      {user.active ? (
                        <Badge variant="success" size="sm" dot>
                          <span>{t("apps.active")}</span>
                        </Badge>
                      ) : (
                        <Badge variant="danger" size="sm" dot>
                          <span>{t("access.disabled")}</span>
                        </Badge>
                      )}
                    </TableCell>
                    <TableCell className="text-xs font-mono text-[var(--muted)] whitespace-nowrap">
                      {new Intl.DateTimeFormat(undefined, {
                        dateStyle: "medium",
                      }).format(user.createdAt)}
                    </TableCell>
                    <TableCell className="text-right">
                      <div className="flex items-center justify-end gap-1">
                        <Button
                          variant="ghost"
                          size="icon-sm"
                          title={t("access.manageApps")}
                          onClick={() => openAppAssignment(user)}
                          icon={<AppWindow size={14} />}
                        />
                        <Button
                          variant="ghost"
                          size="icon-sm"
                          title={t("access.resetPassword")}
                          onClick={() => setResettingUser(user)}
                          icon={<KeyRound size={14} />}
                        />
                        <Button
                          variant="ghost"
                          size="icon-sm"
                          title={t("access.editUser")}
                          onClick={() => setEditingUser(user)}
                          icon={<Settings size={14} />}
                        />
                        {user.id !== currentUserId ? (
                          <Button
                            variant="ghost"
                            size="icon-sm"
                            className="hover:text-[var(--danger)] hover:bg-[var(--danger-subtle)]"
                            title={t("access.deleteUser")}
                            onClick={() => handleDeleteUser(user)}
                            icon={<Trash2 size={14} />}
                          />
                        ) : null}
                      </div>
                    </TableCell>
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
        )}
      </div>

      {/* Modal 1: Create Member Modal */}
      {creatingUser ? (
        <CreateUserModal
          applications={applications}
          canAssignPrivileged={currentUserRoles.includes("Super Admin")}
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
          canAssignPrivileged={currentUserRoles.includes("Super Admin")}
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
            {t("access.appAccessDesc")}
          </p>
          <div style={{ display: "flex", flexDirection: "column", gap: "8px", maxHeight: "300px", overflowY: "auto" }}>
            {applications.length === 0 ? (
              <p className="text-xs text-muted">{t("access.noApps")}</p>
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
  canAssignPrivileged,
  onClose,
  onSuccess,
}: {
  applications: Application[];
  canAssignPrivileged: boolean;
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
              minLength={15}
              placeholder={t("access.min15Chars")}
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
            {canAssignPrivileged ? (
              <>
                <option value="Admin">{t("access.admin")}</option>
                <option value="Super Admin">{t("access.superAdmin")}</option>
              </>
            ) : null}
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
  canAssignPrivileged,
  onClose,
  onSuccess,
}: {
  user: UserSummary;
  canAssignPrivileged: boolean;
  onClose: () => void;
  onSuccess: () => void;
}) {
  const { t } = useTranslation();
  const initialRole = user.roles.includes("Super Admin")
    ? "Super Admin"
    : user.roles.includes("Admin")
      ? "Admin"
      : "User";
  const [selectedRole, setSelectedRole] = useState(initialRole);
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
          ...(canAssignPrivileged ? { role: selectedRole } : {}),
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

        {canAssignPrivileged ? (
          <label className="field">
            <span>{t("access.role")}</span>
            <select
              value={selectedRole}
              onChange={(e) => setSelectedRole(e.target.value)}
            >
              <option value="User">{t("access.user")}</option>
              <option value="Admin">{t("access.admin")}</option>
              <option value="Super Admin">{t("access.superAdmin")}</option>
            </select>
          </label>
        ) : null}

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
              minLength={15}
              placeholder={t("access.min15Chars")}
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
