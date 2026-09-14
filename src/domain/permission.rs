use serde::{Deserialize, Serialize};

pub const PERM_TELEMETRY_EVENTS: &str = "telemetry.events";
pub const PERM_TELEMETRY_METRICS: &str = "telemetry.metrics";
pub const PERM_TELEMETRY_LOGS: &str = "telemetry.logs";
pub const PERM_TELEMETRY_ERRORS: &str = "telemetry.errors";
pub const PERM_TELEMETRY_INGEST: &str = "telemetry.ingest";

pub const OWNER_PERMISSIONS: &[&str] = &["*"];
pub const ADMIN_PERMISSIONS: &[&str] = &[
    "apps.create",
    "apps.read",
    "apps.manage",
    "telemetry.read",
    "telemetry.ingest",
    "alerts.read",
    "alerts.manage",
    "members.read",
    "members.manage",
    "migrations.manage",
    "audit.read",
    "settings.manage",
];
pub const USER_PERMISSIONS: &[&str] = &["apps.create", "telemetry.ingest"];
pub const MANAGER_PERMISSIONS: &[&str] = &[
    "apps.read",
    "apps.manage",
    "telemetry.read",
    "telemetry.ingest",
    "alerts.read",
    "alerts.manage",
];
pub const ANALYST_PERMISSIONS: &[&str] = &[
    "apps.read",
    "telemetry.read",
    "alerts.read",
    "alerts.manage",
];
pub const VIEWER_PERMISSIONS: &[&str] = &["apps.read", "telemetry.read", "alerts.read"];

pub const SUPER_ADMIN_ROLE: &str = "Super Admin";
pub const ADMIN_ROLE: &str = "Admin";
pub const USER_ROLE: &str = "User";
pub const MANAGER_ROLE: &str = "Manager";
pub const ANALYST_ROLE: &str = "Analyst";
pub const VIEWER_ROLE: &str = "Viewer";

const ASSIGNABLE_GLOBAL_ROLES: &[&str] = &[SUPER_ADMIN_ROLE, ADMIN_ROLE, USER_ROLE];
const ASSIGNABLE_APPLICATION_ROLES: &[&str] = &[MANAGER_ROLE, ANALYST_ROLE, VIEWER_ROLE];

#[must_use]
pub fn is_assignable_global_role(role: &str) -> bool {
    ASSIGNABLE_GLOBAL_ROLES.contains(&role)
}

#[must_use]
pub fn is_assignable_application_role(role: &str) -> bool {
    ASSIGNABLE_APPLICATION_ROLES.contains(&role)
}

#[must_use]
pub fn is_privileged_global_role(role: &str) -> bool {
    role == SUPER_ADMIN_ROLE || role == ADMIN_ROLE
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PermissionGrant {
    pub permissions: Vec<String>,
    pub application_id: Option<String>,
}

impl PermissionGrant {
    #[must_use]
    pub fn allows(&self, permission: &str, application_id: Option<&str>) -> bool {
        let permission_matches = self
            .permissions
            .iter()
            .any(|candidate| candidate == "*" || candidate == permission);
        let scope_matches = self
            .application_id
            .as_deref()
            .is_none_or(|scope| Some(scope) == application_id);
        permission_matches && scope_matches
    }
}

#[cfg(test)]
mod tests {
    use super::PermissionGrant;

    #[test]
    fn application_grant_rejects_other_application() {
        let grant = PermissionGrant {
            permissions: vec!["telemetry.read".into()],
            application_id: Some("one".into()),
        };
        assert!(!grant.allows("telemetry.read", Some("two")));
    }

    #[test]
    fn wildcard_allows_every_permission() {
        let grant = PermissionGrant {
            permissions: vec!["*".into()],
            application_id: None,
        };
        assert!(grant.allows("settings.manage", None));
    }

    #[test]
    fn application_roles_cannot_be_used_as_global_roles() {
        assert!(super::is_assignable_application_role("Manager"));
        assert!(super::is_assignable_application_role("Analyst"));
        assert!(super::is_assignable_application_role("Viewer"));
        assert!(!super::is_assignable_application_role("Super Admin"));
        assert!(!super::is_assignable_application_role("Admin"));
        assert!(super::is_assignable_global_role("Super Admin"));
        assert!(super::is_assignable_global_role("User"));
        assert!(!super::is_assignable_global_role("Manager"));
    }
}
