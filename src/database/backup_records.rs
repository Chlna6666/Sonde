use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupUser {
    pub id: String,
    pub email: String,
    pub username: String,
    pub password_hash: String,
    pub locale: String,
    pub active: bool,
    pub created_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupRole {
    pub id: String,
    pub name: String,
    pub builtin: bool,
    pub permissions: String,
    pub created_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupRoleBinding {
    pub id: String,
    pub user_id: String,
    pub role_id: String,
    pub application_id: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupApplication {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub retention_days: i32,
    pub owner_user_id: Option<String>,
    pub is_public: bool,
    pub description: Option<String>,
    pub github_url: Option<String>,
    pub website_url: Option<String>,
    pub custom_header: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupEnvironment {
    pub id: String,
    pub application_id: String,
    pub name: String,
    pub slug: String,
    pub created_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupApiKey {
    pub id: String,
    pub application_id: String,
    pub environment_id: String,
    pub name: String,
    pub key_hash: String,
    pub key_prefix: String,
    pub scopes: String,
    pub expires_at: Option<i64>,
    pub last_used_at: Option<i64>,
    pub revoked_at: Option<i64>,
    pub created_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupAlertRule {
    pub id: String,
    pub application_id: String,
    pub name: String,
    pub enabled: bool,
    pub source_kind: String,
    pub query_json: String,
    pub window_minutes: i32,
    pub cooldown_seconds: i32,
    pub last_state: String,
    pub last_evaluated_at: Option<i64>,
    pub created_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupNotificationChannel {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub config_json: String,
    pub enabled: bool,
    pub created_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupAuditLog {
    pub id: String,
    pub actor_user_id: Option<String>,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<String>,
    pub metadata: String,
    pub created_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupEvent {
    pub id: String,
    pub application_id: String,
    pub environment_id: String,
    pub name: String,
    pub timestamp: i64,
    pub day: String,
    pub anonymous_id: Option<String>,
    pub session_id: Option<String>,
    pub app_version: Option<String>,
    pub launcher_version: Option<String>,
    pub os: Option<String>,
    pub attributes: String,
    pub dedupe_key: Option<String>,
    pub received_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupLog {
    pub id: String,
    pub application_id: String,
    pub environment_id: String,
    pub level: String,
    pub message: String,
    pub logger: Option<String>,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub timestamp: i64,
    pub attributes: String,
    pub received_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupDailyAggregate {
    pub id: String,
    pub application_id: String,
    pub environment_id: String,
    pub day: String,
    pub kind: String,
    pub dimension: String,
    pub dimension_value: String,
    pub count: i64,
    pub sum: Option<f64>,
    pub updated_at: i64,
}
