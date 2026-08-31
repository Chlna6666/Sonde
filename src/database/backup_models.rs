use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr,
    sea_query::{Alias, Expr, ExprTrait, Query, Value},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use super::query::{insert, insert_batch};

// =========================================================================
// Single Application Export / Import Structures
// =========================================================================

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SingleAppExport {
    pub format_version: String,
    pub export_type: String,
    pub exported_at: i64,
    pub application: ExportedApplication,
    pub environments: Vec<ExportedEnvironment>,
    pub api_keys: Vec<ExportedApiKey>,
    pub telemetry: ExportedTelemetry,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedApplication {
    pub name: String,
    pub slug: String,
    pub retention_days: i32,
    pub is_public: bool,
    pub description: Option<String>,
    pub github_url: Option<String>,
    pub website_url: Option<String>,
    pub custom_header: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedEnvironment {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub created_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedApiKey {
    pub id: String,
    pub environment_id: String,
    pub name: String,
    pub key_hash: String,
    pub key_prefix: String,
    pub scopes: Vec<String>,
    pub expires_at: Option<i64>,
    pub last_used_at: Option<i64>,
    pub revoked_at: Option<i64>,
    pub created_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedTelemetry {
    pub events: Vec<ExportedEvent>,
    pub metric_points: Vec<ExportedMetricPoint>,
    pub logs: Vec<ExportedLog>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedEvent {
    pub environment_id: String,
    pub name: String,
    pub timestamp: i64,
    pub day: String,
    pub anonymous_id: Option<String>,
    pub session_id: Option<String>,
    pub app_version: Option<String>,
    pub launcher_version: Option<String>,
    pub os: Option<String>,
    pub attributes: serde_json::Value,
    pub dedupe_key: Option<String>,
    pub received_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedMetricPoint {
    pub environment_id: String,
    pub name: String,
    pub metric_type: String,
    pub value: f64,
    pub unit: Option<String>,
    pub timestamp: i64,
    pub attributes: serde_json::Value,
    pub received_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedLog {
    pub environment_id: String,
    pub level: String,
    pub message: String,
    pub logger: Option<String>,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub timestamp: i64,
    pub attributes: serde_json::Value,
    pub received_at: i64,
}

// =========================================================================
// Full System Backup Structures
// =========================================================================

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FullSystemBackup {
    pub format_version: String,
    pub backup_type: String,
    pub exported_at: i64,
    pub server_version: String,
    pub users: Vec<BackupUser>,
    pub roles: Vec<BackupRole>,
    pub role_bindings: Vec<BackupRoleBinding>,
    pub applications: Vec<BackupApplication>,
    pub environments: Vec<BackupEnvironment>,
    pub api_keys: Vec<BackupApiKey>,
    pub alert_rules: Vec<BackupAlertRule>,
    pub notification_channels: Vec<BackupNotificationChannel>,
    pub audit_log: Vec<BackupAuditLog>,
    pub events: Vec<BackupEvent>,
    pub metric_points: Vec<BackupMetricPoint>,
    pub logs: Vec<BackupLog>,
    pub daily_aggregates: Vec<BackupDailyAggregate>,
}

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
pub struct BackupMetricPoint {
    pub id: String,
    pub application_id: String,
    pub environment_id: String,
    pub name: String,
    pub metric_type: String,
    pub value: f64,
    pub unit: Option<String>,
    pub timestamp: i64,
    pub attributes: String,
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

// =========================================================================
// Single Application Export / Import Implementation
// =========================================================================

pub async fn export_single_application(
    database: &DatabaseConnection,
    application_id: &str,
) -> Result<Option<SingleAppExport>, DbErr> {
    let app_query = Query::select()
        .columns(
            [
                "id",
                "name",
                "slug",
                "retention_days",
                "is_public",
                "description",
                "github_url",
                "website_url",
                "custom_header",
                "created_at",
            ]
            .map(Alias::new),
        )
        .from(Alias::new("applications"))
        .and_where(Expr::col(Alias::new("id")).eq(application_id))
        .limit(1)
        .to_owned();

    let Some(app_row) = database.query_one(&app_query).await? else {
        return Ok(None);
    };

    let application = ExportedApplication {
        name: app_row.try_get("", "name")?,
        slug: app_row.try_get("", "slug")?,
        retention_days: app_row.try_get("", "retention_days")?,
        is_public: app_row.try_get("", "is_public").unwrap_or(false),
        description: app_row.try_get("", "description").ok(),
        github_url: app_row.try_get("", "github_url").ok(),
        website_url: app_row.try_get("", "website_url").ok(),
        custom_header: app_row.try_get("", "custom_header").ok(),
        created_at: app_row.try_get("", "created_at")?,
    };

    // Environments
    let env_query = Query::select()
        .columns(["id", "name", "slug", "created_at"].map(Alias::new))
        .from(Alias::new("environments"))
        .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
        .order_by(Alias::new("created_at"), sea_orm::Order::Asc)
        .to_owned();

    let env_rows = database.query_all(&env_query).await?;
    let environments = env_rows
        .into_iter()
        .map(|r| {
            Ok(ExportedEnvironment {
                id: r.try_get("", "id")?,
                name: r.try_get("", "name")?,
                slug: r.try_get("", "slug")?,
                created_at: r.try_get("", "created_at")?,
            })
        })
        .collect::<Result<Vec<_>, DbErr>>()?;

    // API Keys
    let keys_query = Query::select()
        .columns(
            [
                "id",
                "environment_id",
                "name",
                "key_hash",
                "key_prefix",
                "scopes",
                "expires_at",
                "last_used_at",
                "revoked_at",
                "created_at",
            ]
            .map(Alias::new),
        )
        .from(Alias::new("api_keys"))
        .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
        .to_owned();

    let key_rows = database.query_all(&keys_query).await?;
    let api_keys = key_rows
        .into_iter()
        .map(|r| {
            let scopes_raw: String = r.try_get("", "scopes")?;
            let scopes: Vec<String> = serde_json::from_str(&scopes_raw).unwrap_or_default();
            Ok(ExportedApiKey {
                id: r.try_get("", "id")?,
                environment_id: r.try_get("", "environment_id")?,
                name: r.try_get("", "name")?,
                key_hash: r.try_get("", "key_hash")?,
                key_prefix: r.try_get("", "key_prefix")?,
                scopes,
                expires_at: r.try_get("", "expires_at").ok(),
                last_used_at: r.try_get("", "last_used_at").ok(),
                revoked_at: r.try_get("", "revoked_at").ok(),
                created_at: r.try_get("", "created_at")?,
            })
        })
        .collect::<Result<Vec<_>, DbErr>>()?;

    // Events
    let events_query = Query::select()
        .columns(
            [
                "environment_id",
                "name",
                "timestamp",
                "day",
                "anonymous_id",
                "session_id",
                "app_version",
                "launcher_version",
                "os",
                "attributes",
                "dedupe_key",
                "received_at",
            ]
            .map(Alias::new),
        )
        .from(Alias::new("events"))
        .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
        .order_by(Alias::new("timestamp"), sea_orm::Order::Asc)
        .to_owned();

    let event_rows = database.query_all(&events_query).await?;
    let events = event_rows
        .into_iter()
        .map(|r| {
            let attrs_raw: String = r
                .try_get("", "attributes")
                .unwrap_or_else(|_| "{}".to_string());
            let attributes = serde_json::from_str(&attrs_raw).unwrap_or(serde_json::json!({}));
            Ok(ExportedEvent {
                environment_id: r.try_get("", "environment_id")?,
                name: r.try_get("", "name")?,
                timestamp: r.try_get("", "timestamp")?,
                day: r.try_get("", "day")?,
                anonymous_id: r.try_get("", "anonymous_id").ok(),
                session_id: r.try_get("", "session_id").ok(),
                app_version: r.try_get("", "app_version").ok(),
                launcher_version: r.try_get("", "launcher_version").ok(),
                os: r.try_get("", "os").ok(),
                attributes,
                dedupe_key: r.try_get("", "dedupe_key").ok(),
                received_at: r.try_get("", "received_at")?,
            })
        })
        .collect::<Result<Vec<_>, DbErr>>()?;

    // Metric Points
    let metrics_query = Query::select()
        .columns(
            [
                "environment_id",
                "name",
                "metric_type",
                "value",
                "unit",
                "timestamp",
                "attributes",
                "received_at",
            ]
            .map(Alias::new),
        )
        .from(Alias::new("metric_points"))
        .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
        .order_by(Alias::new("timestamp"), sea_orm::Order::Asc)
        .to_owned();

    let metric_rows = database.query_all(&metrics_query).await?;
    let metric_points = metric_rows
        .into_iter()
        .map(|r| {
            let attrs_raw: String = r
                .try_get("", "attributes")
                .unwrap_or_else(|_| "{}".to_string());
            let attributes = serde_json::from_str(&attrs_raw).unwrap_or(serde_json::json!({}));
            Ok(ExportedMetricPoint {
                environment_id: r.try_get("", "environment_id")?,
                name: r.try_get("", "name")?,
                metric_type: r.try_get("", "metric_type")?,
                value: r.try_get("", "value")?,
                unit: r.try_get("", "unit").ok(),
                timestamp: r.try_get("", "timestamp")?,
                attributes,
                received_at: r.try_get("", "received_at")?,
            })
        })
        .collect::<Result<Vec<_>, DbErr>>()?;

    // Logs
    let logs_query = Query::select()
        .columns(
            [
                "environment_id",
                "level",
                "message",
                "logger",
                "trace_id",
                "span_id",
                "timestamp",
                "attributes",
                "received_at",
            ]
            .map(Alias::new),
        )
        .from(Alias::new("logs"))
        .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
        .order_by(Alias::new("timestamp"), sea_orm::Order::Asc)
        .to_owned();

    let log_rows = database.query_all(&logs_query).await?;
    let logs = log_rows
        .into_iter()
        .map(|r| {
            let attrs_raw: String = r
                .try_get("", "attributes")
                .unwrap_or_else(|_| "{}".to_string());
            let attributes = serde_json::from_str(&attrs_raw).unwrap_or(serde_json::json!({}));
            Ok(ExportedLog {
                environment_id: r.try_get("", "environment_id")?,
                level: r.try_get("", "level")?,
                message: r.try_get("", "message")?,
                logger: r.try_get("", "logger").ok(),
                trace_id: r.try_get("", "trace_id").ok(),
                span_id: r.try_get("", "span_id").ok(),
                timestamp: r.try_get("", "timestamp")?,
                attributes,
                received_at: r.try_get("", "received_at")?,
            })
        })
        .collect::<Result<Vec<_>, DbErr>>()?;

    Ok(Some(SingleAppExport {
        format_version: "1.0".into(),
        export_type: "sonde_application".into(),
        exported_at: chrono::Utc::now().timestamp_millis(),
        application,
        environments,
        api_keys,
        telemetry: ExportedTelemetry {
            events,
            metric_points,
            logs,
        },
    }))
}

pub async fn import_single_application(
    database: &DatabaseConnection,
    owner_user_id: Option<&str>,
    payload: SingleAppExport,
) -> Result<String, DbErr> {
    let now = chrono::Utc::now().timestamp_millis();
    let new_app_id = Uuid::now_v7().to_string();

    // Determine unique slug
    let mut slug = payload.application.slug.clone();
    let check_query = Query::select()
        .column(Alias::new("id"))
        .from(Alias::new("applications"))
        .and_where(Expr::col(Alias::new("slug")).eq(&slug))
        .limit(1)
        .to_owned();
    if database.query_one(&check_query).await?.is_some() {
        slug = format!("{}-imported-{}", slug, &new_app_id[..6]);
    }

    // Insert Application
    insert(
        database,
        "applications",
        &[
            "id",
            "name",
            "slug",
            "retention_days",
            "owner_user_id",
            "is_public",
            "description",
            "github_url",
            "website_url",
            "custom_header",
            "created_at",
        ],
        vec![
            new_app_id.clone().into(),
            payload.application.name.into(),
            slug.into(),
            payload.application.retention_days.into(),
            owner_user_id.map(Into::into).unwrap_or(Value::String(None)),
            payload.application.is_public.into(),
            payload
                .application
                .description
                .map(Into::into)
                .unwrap_or(Value::String(None)),
            payload
                .application
                .github_url
                .map(Into::into)
                .unwrap_or(Value::String(None)),
            payload
                .application
                .website_url
                .map(Into::into)
                .unwrap_or(Value::String(None)),
            payload
                .application
                .custom_header
                .map(Into::into)
                .unwrap_or(Value::String(None)),
            now.into(),
        ],
    )
    .await?;

    // Map old Environment IDs to new Environment IDs
    let mut env_id_map: HashMap<String, String> = HashMap::new();
    for env in payload.environments {
        let new_env_id = Uuid::now_v7().to_string();
        insert(
            database,
            "environments",
            &["id", "application_id", "name", "slug", "created_at"],
            vec![
                new_env_id.clone().into(),
                new_app_id.clone().into(),
                env.name.into(),
                env.slug.into(),
                now.into(),
            ],
        )
        .await?;
        env_id_map.insert(env.id, new_env_id);
    }

    // Default environment fallback if none was imported
    let default_env_id = if env_id_map.is_empty() {
        let fallback_id = Uuid::now_v7().to_string();
        insert(
            database,
            "environments",
            &["id", "application_id", "name", "slug", "created_at"],
            vec![
                fallback_id.clone().into(),
                new_app_id.clone().into(),
                "Production".into(),
                "production".into(),
                now.into(),
            ],
        )
        .await?;
        fallback_id
    } else {
        env_id_map
            .values()
            .next()
            .cloned()
            .unwrap_or_else(|| Uuid::now_v7().to_string())
    };

    // Insert API Keys
    for key in payload.api_keys {
        let mapped_env_id = env_id_map
            .get(&key.environment_id)
            .cloned()
            .unwrap_or_else(|| default_env_id.clone());
        let new_key_id = Uuid::now_v7().to_string();
        let scopes_json = serde_json::to_string(&key.scopes).unwrap_or_else(|_| "{}".to_string());
        let _ = insert(
            database,
            "api_keys",
            &[
                "id",
                "application_id",
                "environment_id",
                "name",
                "key_hash",
                "key_prefix",
                "scopes",
                "expires_at",
                "last_used_at",
                "revoked_at",
                "created_at",
            ],
            vec![
                new_key_id.into(),
                new_app_id.clone().into(),
                mapped_env_id.into(),
                key.name.into(),
                key.key_hash.into(),
                key.key_prefix.into(),
                scopes_json.into(),
                key.expires_at
                    .map(Into::into)
                    .unwrap_or(Value::BigInt(None)),
                key.last_used_at
                    .map(Into::into)
                    .unwrap_or(Value::BigInt(None)),
                key.revoked_at
                    .map(Into::into)
                    .unwrap_or(Value::BigInt(None)),
                now.into(),
            ],
        )
        .await;
    }

    // Insert Telemetry Events
    for event in payload.telemetry.events {
        let mapped_env_id = env_id_map
            .get(&event.environment_id)
            .cloned()
            .unwrap_or_else(|| default_env_id.clone());
        let event_id = Uuid::now_v7().to_string();
        let attrs_str =
            serde_json::to_string(&event.attributes).unwrap_or_else(|_| "{}".to_string());
        let _ = insert(
            database,
            "events",
            &[
                "id",
                "application_id",
                "environment_id",
                "name",
                "timestamp",
                "day",
                "anonymous_id",
                "session_id",
                "app_version",
                "launcher_version",
                "os",
                "attributes",
                "dedupe_key",
                "received_at",
            ],
            vec![
                event_id.into(),
                new_app_id.clone().into(),
                mapped_env_id.into(),
                event.name.into(),
                event.timestamp.into(),
                event.day.into(),
                event
                    .anonymous_id
                    .map(Into::into)
                    .unwrap_or(Value::String(None)),
                event
                    .session_id
                    .map(Into::into)
                    .unwrap_or(Value::String(None)),
                event
                    .app_version
                    .map(Into::into)
                    .unwrap_or(Value::String(None)),
                event
                    .launcher_version
                    .map(Into::into)
                    .unwrap_or(Value::String(None)),
                event.os.map(Into::into).unwrap_or(Value::String(None)),
                attrs_str.into(),
                event
                    .dedupe_key
                    .map(Into::into)
                    .unwrap_or(Value::String(None)),
                event.received_at.into(),
            ],
        )
        .await;
    }

    // Insert Metric Points
    for mp in payload.telemetry.metric_points {
        let mapped_env_id = env_id_map
            .get(&mp.environment_id)
            .cloned()
            .unwrap_or_else(|| default_env_id.clone());
        let mp_id = Uuid::now_v7().to_string();
        let attrs_str = serde_json::to_string(&mp.attributes).unwrap_or_else(|_| "{}".to_string());
        let _ = insert(
            database,
            "metric_points",
            &[
                "id",
                "application_id",
                "environment_id",
                "name",
                "metric_type",
                "value",
                "unit",
                "timestamp",
                "attributes",
                "received_at",
            ],
            vec![
                mp_id.into(),
                new_app_id.clone().into(),
                mapped_env_id.into(),
                mp.name.into(),
                mp.metric_type.into(),
                mp.value.into(),
                mp.unit.map(Into::into).unwrap_or(Value::String(None)),
                mp.timestamp.into(),
                attrs_str.into(),
                mp.received_at.into(),
            ],
        )
        .await;
    }

    // Insert Logs
    for log in payload.telemetry.logs {
        let mapped_env_id = env_id_map
            .get(&log.environment_id)
            .cloned()
            .unwrap_or_else(|| default_env_id.clone());
        let log_id = Uuid::now_v7().to_string();
        let attrs_str = serde_json::to_string(&log.attributes).unwrap_or_else(|_| "{}".to_string());
        let _ = insert(
            database,
            "logs",
            &[
                "id",
                "application_id",
                "environment_id",
                "level",
                "message",
                "logger",
                "trace_id",
                "span_id",
                "timestamp",
                "attributes",
                "received_at",
            ],
            vec![
                log_id.into(),
                new_app_id.clone().into(),
                mapped_env_id.into(),
                log.level.into(),
                log.message.into(),
                log.logger.map(Into::into).unwrap_or(Value::String(None)),
                log.trace_id.map(Into::into).unwrap_or(Value::String(None)),
                log.span_id.map(Into::into).unwrap_or(Value::String(None)),
                log.timestamp.into(),
                attrs_str.into(),
                log.received_at.into(),
            ],
        )
        .await;
    }

    Ok(new_app_id)
}

// =========================================================================
// Full System Backup / Restore Implementation
// =========================================================================

pub async fn export_full_system(database: &DatabaseConnection) -> Result<FullSystemBackup, DbErr> {
    // Users
    let users = database
        .query_all(
            &Query::select()
                .columns(
                    [
                        "id",
                        "email",
                        "username",
                        "password_hash",
                        "locale",
                        "active",
                        "created_at",
                    ]
                    .map(Alias::new),
                )
                .from(Alias::new("users"))
                .to_owned(),
        )
        .await?
        .into_iter()
        .map(|r| {
            Ok(BackupUser {
                id: r.try_get("", "id")?,
                email: r.try_get("", "email")?,
                username: r.try_get("", "username")?,
                password_hash: r.try_get("", "password_hash")?,
                locale: r.try_get("", "locale")?,
                active: r.try_get("", "active")?,
                created_at: r.try_get("", "created_at")?,
            })
        })
        .collect::<Result<Vec<_>, DbErr>>()?;

    // Roles
    let roles = database
        .query_all(
            &Query::select()
                .columns(["id", "name", "builtin", "permissions", "created_at"].map(Alias::new))
                .from(Alias::new("roles"))
                .to_owned(),
        )
        .await?
        .into_iter()
        .map(|r| {
            Ok(BackupRole {
                id: r.try_get("", "id")?,
                name: r.try_get("", "name")?,
                builtin: r.try_get("", "builtin")?,
                permissions: r.try_get("", "permissions")?,
                created_at: r.try_get("", "created_at")?,
            })
        })
        .collect::<Result<Vec<_>, DbErr>>()?;

    // Role Bindings
    let role_bindings = database
        .query_all(
            &Query::select()
                .columns(
                    ["id", "user_id", "role_id", "application_id", "created_at"].map(Alias::new),
                )
                .from(Alias::new("role_bindings"))
                .to_owned(),
        )
        .await?
        .into_iter()
        .map(|r| {
            Ok(BackupRoleBinding {
                id: r.try_get("", "id")?,
                user_id: r.try_get("", "user_id")?,
                role_id: r.try_get("", "role_id")?,
                application_id: r.try_get("", "application_id").ok(),
                created_at: r.try_get("", "created_at")?,
            })
        })
        .collect::<Result<Vec<_>, DbErr>>()?;

    // Applications
    let applications = database
        .query_all(
            &Query::select()
                .columns(
                    [
                        "id",
                        "name",
                        "slug",
                        "retention_days",
                        "owner_user_id",
                        "is_public",
                        "description",
                        "github_url",
                        "website_url",
                        "custom_header",
                        "created_at",
                    ]
                    .map(Alias::new),
                )
                .from(Alias::new("applications"))
                .to_owned(),
        )
        .await?
        .into_iter()
        .map(|r| {
            Ok(BackupApplication {
                id: r.try_get("", "id")?,
                name: r.try_get("", "name")?,
                slug: r.try_get("", "slug")?,
                retention_days: r.try_get("", "retention_days")?,
                owner_user_id: r.try_get("", "owner_user_id").ok(),
                is_public: r.try_get("", "is_public").unwrap_or(false),
                description: r.try_get("", "description").ok(),
                github_url: r.try_get("", "github_url").ok(),
                website_url: r.try_get("", "website_url").ok(),
                custom_header: r.try_get("", "custom_header").ok(),
                created_at: r.try_get("", "created_at")?,
            })
        })
        .collect::<Result<Vec<_>, DbErr>>()?;

    // Environments
    let environments = database
        .query_all(
            &Query::select()
                .columns(["id", "application_id", "name", "slug", "created_at"].map(Alias::new))
                .from(Alias::new("environments"))
                .to_owned(),
        )
        .await?
        .into_iter()
        .map(|r| {
            Ok(BackupEnvironment {
                id: r.try_get("", "id")?,
                application_id: r.try_get("", "application_id")?,
                name: r.try_get("", "name")?,
                slug: r.try_get("", "slug")?,
                created_at: r.try_get("", "created_at")?,
            })
        })
        .collect::<Result<Vec<_>, DbErr>>()?;

    // API Keys
    let api_keys = database
        .query_all(
            &Query::select()
                .columns(
                    [
                        "id",
                        "application_id",
                        "environment_id",
                        "name",
                        "key_hash",
                        "key_prefix",
                        "scopes",
                        "expires_at",
                        "last_used_at",
                        "revoked_at",
                        "created_at",
                    ]
                    .map(Alias::new),
                )
                .from(Alias::new("api_keys"))
                .to_owned(),
        )
        .await?
        .into_iter()
        .map(|r| {
            Ok(BackupApiKey {
                id: r.try_get("", "id")?,
                application_id: r.try_get("", "application_id")?,
                environment_id: r.try_get("", "environment_id")?,
                name: r.try_get("", "name")?,
                key_hash: r.try_get("", "key_hash")?,
                key_prefix: r.try_get("", "key_prefix")?,
                scopes: r.try_get("", "scopes")?,
                expires_at: r.try_get("", "expires_at").ok(),
                last_used_at: r.try_get("", "last_used_at").ok(),
                revoked_at: r.try_get("", "revoked_at").ok(),
                created_at: r.try_get("", "created_at")?,
            })
        })
        .collect::<Result<Vec<_>, DbErr>>()?;

    // Alert Rules
    let alert_rules = database
        .query_all(
            &Query::select()
                .columns(
                    [
                        "id",
                        "application_id",
                        "name",
                        "enabled",
                        "source_kind",
                        "query_json",
                        "window_minutes",
                        "cooldown_seconds",
                        "last_state",
                        "last_evaluated_at",
                        "created_at",
                    ]
                    .map(Alias::new),
                )
                .from(Alias::new("alert_rules"))
                .to_owned(),
        )
        .await?
        .into_iter()
        .map(|r| {
            Ok(BackupAlertRule {
                id: r.try_get("", "id")?,
                application_id: r.try_get("", "application_id")?,
                name: r.try_get("", "name")?,
                enabled: r.try_get("", "enabled")?,
                source_kind: r.try_get("", "source_kind")?,
                query_json: r.try_get("", "query_json")?,
                window_minutes: r.try_get("", "window_minutes")?,
                cooldown_seconds: r.try_get("", "cooldown_seconds")?,
                last_state: r.try_get("", "last_state")?,
                last_evaluated_at: r.try_get("", "last_evaluated_at").ok(),
                created_at: r.try_get("", "created_at")?,
            })
        })
        .collect::<Result<Vec<_>, DbErr>>()?;

    // Notification Channels
    let notification_channels = database
        .query_all(
            &Query::select()
                .columns(
                    ["id", "name", "kind", "config_json", "enabled", "created_at"].map(Alias::new),
                )
                .from(Alias::new("notification_channels"))
                .to_owned(),
        )
        .await?
        .into_iter()
        .map(|r| {
            Ok(BackupNotificationChannel {
                id: r.try_get("", "id")?,
                name: r.try_get("", "name")?,
                kind: r.try_get("", "kind")?,
                config_json: r.try_get("", "config_json")?,
                enabled: r.try_get("", "enabled")?,
                created_at: r.try_get("", "created_at")?,
            })
        })
        .collect::<Result<Vec<_>, DbErr>>()?;

    // Audit Log
    let audit_log = database
        .query_all(
            &Query::select()
                .columns(
                    [
                        "id",
                        "actor_user_id",
                        "action",
                        "resource_type",
                        "resource_id",
                        "metadata",
                        "created_at",
                    ]
                    .map(Alias::new),
                )
                .from(Alias::new("audit_log"))
                .to_owned(),
        )
        .await?
        .into_iter()
        .map(|r| {
            Ok(BackupAuditLog {
                id: r.try_get("", "id")?,
                actor_user_id: r.try_get("", "actor_user_id").ok(),
                action: r.try_get("", "action")?,
                resource_type: r.try_get("", "resource_type")?,
                resource_id: r.try_get("", "resource_id").ok(),
                metadata: r.try_get("", "metadata")?,
                created_at: r.try_get("", "created_at")?,
            })
        })
        .collect::<Result<Vec<_>, DbErr>>()?;

    // Events
    let events = database
        .query_all(
            &Query::select()
                .columns(
                    [
                        "id",
                        "application_id",
                        "environment_id",
                        "name",
                        "timestamp",
                        "day",
                        "anonymous_id",
                        "session_id",
                        "app_version",
                        "launcher_version",
                        "os",
                        "attributes",
                        "dedupe_key",
                        "received_at",
                    ]
                    .map(Alias::new),
                )
                .from(Alias::new("events"))
                .to_owned(),
        )
        .await?
        .into_iter()
        .map(|r| {
            Ok(BackupEvent {
                id: r.try_get("", "id")?,
                application_id: r.try_get("", "application_id")?,
                environment_id: r.try_get("", "environment_id")?,
                name: r.try_get("", "name")?,
                timestamp: r.try_get("", "timestamp")?,
                day: r.try_get("", "day")?,
                anonymous_id: r.try_get("", "anonymous_id").ok(),
                session_id: r.try_get("", "session_id").ok(),
                app_version: r.try_get("", "app_version").ok(),
                launcher_version: r.try_get("", "launcher_version").ok(),
                os: r.try_get("", "os").ok(),
                attributes: r.try_get("", "attributes")?,
                dedupe_key: r.try_get("", "dedupe_key").ok(),
                received_at: r.try_get("", "received_at")?,
            })
        })
        .collect::<Result<Vec<_>, DbErr>>()?;

    // Metric Points
    let metric_points = database
        .query_all(
            &Query::select()
                .columns(
                    [
                        "id",
                        "application_id",
                        "environment_id",
                        "name",
                        "metric_type",
                        "value",
                        "unit",
                        "timestamp",
                        "attributes",
                        "received_at",
                    ]
                    .map(Alias::new),
                )
                .from(Alias::new("metric_points"))
                .to_owned(),
        )
        .await?
        .into_iter()
        .map(|r| {
            Ok(BackupMetricPoint {
                id: r.try_get("", "id")?,
                application_id: r.try_get("", "application_id")?,
                environment_id: r.try_get("", "environment_id")?,
                name: r.try_get("", "name")?,
                metric_type: r.try_get("", "metric_type")?,
                value: r.try_get("", "value")?,
                unit: r.try_get("", "unit").ok(),
                timestamp: r.try_get("", "timestamp")?,
                attributes: r.try_get("", "attributes")?,
                received_at: r.try_get("", "received_at")?,
            })
        })
        .collect::<Result<Vec<_>, DbErr>>()?;

    // Logs
    let logs = database
        .query_all(
            &Query::select()
                .columns(
                    [
                        "id",
                        "application_id",
                        "environment_id",
                        "level",
                        "message",
                        "logger",
                        "trace_id",
                        "span_id",
                        "timestamp",
                        "attributes",
                        "received_at",
                    ]
                    .map(Alias::new),
                )
                .from(Alias::new("logs"))
                .to_owned(),
        )
        .await?
        .into_iter()
        .map(|r| {
            Ok(BackupLog {
                id: r.try_get("", "id")?,
                application_id: r.try_get("", "application_id")?,
                environment_id: r.try_get("", "environment_id")?,
                level: r.try_get("", "level")?,
                message: r.try_get("", "message")?,
                logger: r.try_get("", "logger").ok(),
                trace_id: r.try_get("", "trace_id").ok(),
                span_id: r.try_get("", "span_id").ok(),
                timestamp: r.try_get("", "timestamp")?,
                attributes: r.try_get("", "attributes")?,
                received_at: r.try_get("", "received_at")?,
            })
        })
        .collect::<Result<Vec<_>, DbErr>>()?;

    // Daily Aggregates
    let daily_aggregates = database
        .query_all(
            &Query::select()
                .columns(
                    [
                        "id",
                        "application_id",
                        "environment_id",
                        "day",
                        "kind",
                        "dimension",
                        "dimension_value",
                        "count",
                        "sum",
                        "updated_at",
                    ]
                    .map(Alias::new),
                )
                .from(Alias::new("daily_aggregates"))
                .to_owned(),
        )
        .await?
        .into_iter()
        .map(|r| {
            Ok(BackupDailyAggregate {
                id: r.try_get("", "id")?,
                application_id: r.try_get("", "application_id")?,
                environment_id: r.try_get("", "environment_id")?,
                day: r.try_get("", "day")?,
                kind: r.try_get("", "kind")?,
                dimension: r.try_get("", "dimension")?,
                dimension_value: r.try_get("", "dimension_value")?,
                count: r.try_get("", "count")?,
                sum: r.try_get("", "sum").ok(),
                updated_at: r.try_get("", "updated_at")?,
            })
        })
        .collect::<Result<Vec<_>, DbErr>>()?;

    Ok(FullSystemBackup {
        format_version: "1.0".into(),
        backup_type: "sonde_full_backup".into(),
        exported_at: chrono::Utc::now().timestamp_millis(),
        server_version: env!("CARGO_PKG_VERSION").into(),
        users,
        roles,
        role_bindings,
        applications,
        environments,
        api_keys,
        alert_rules,
        notification_channels,
        audit_log,
        events,
        metric_points,
        logs,
        daily_aggregates,
    })
}

pub async fn restore_full_system(
    database: &DatabaseConnection,
    backup: FullSystemBackup,
) -> Result<(), DbErr> {
    // 1. Roles
    for r in backup.roles {
        let _ = insert(
            database,
            "roles",
            &["id", "name", "builtin", "permissions", "created_at"],
            vec![
                r.id.into(),
                r.name.into(),
                r.builtin.into(),
                r.permissions.into(),
                r.created_at.into(),
            ],
        )
        .await;
    }

    // 2. Users
    for u in backup.users {
        let _ = insert(
            database,
            "users",
            &[
                "id",
                "email",
                "username",
                "password_hash",
                "locale",
                "active",
                "created_at",
            ],
            vec![
                u.id.into(),
                u.email.into(),
                u.username.into(),
                u.password_hash.into(),
                u.locale.into(),
                u.active.into(),
                u.created_at.into(),
            ],
        )
        .await;
    }

    // 3. Applications
    for a in backup.applications {
        let _ = insert(
            database,
            "applications",
            &[
                "id",
                "name",
                "slug",
                "retention_days",
                "owner_user_id",
                "is_public",
                "description",
                "github_url",
                "website_url",
                "custom_header",
                "created_at",
            ],
            vec![
                a.id.into(),
                a.name.into(),
                a.slug.into(),
                a.retention_days.into(),
                a.owner_user_id
                    .map(Into::into)
                    .unwrap_or(Value::String(None)),
                a.is_public.into(),
                a.description.map(Into::into).unwrap_or(Value::String(None)),
                a.github_url.map(Into::into).unwrap_or(Value::String(None)),
                a.website_url.map(Into::into).unwrap_or(Value::String(None)),
                a.custom_header
                    .map(Into::into)
                    .unwrap_or(Value::String(None)),
                a.created_at.into(),
            ],
        )
        .await;
    }

    // 4. Role Bindings
    for rb in backup.role_bindings {
        let _ = insert(
            database,
            "role_bindings",
            &["id", "user_id", "role_id", "application_id", "created_at"],
            vec![
                rb.id.into(),
                rb.user_id.into(),
                rb.role_id.into(),
                rb.application_id
                    .map(Into::into)
                    .unwrap_or(Value::String(None)),
                rb.created_at.into(),
            ],
        )
        .await;
    }

    // 5. Environments
    for env in backup.environments {
        let _ = insert(
            database,
            "environments",
            &["id", "application_id", "name", "slug", "created_at"],
            vec![
                env.id.into(),
                env.application_id.into(),
                env.name.into(),
                env.slug.into(),
                env.created_at.into(),
            ],
        )
        .await;
    }

    // 6. API Keys
    for k in backup.api_keys {
        let _ = insert(
            database,
            "api_keys",
            &[
                "id",
                "application_id",
                "environment_id",
                "name",
                "key_hash",
                "key_prefix",
                "scopes",
                "expires_at",
                "last_used_at",
                "revoked_at",
                "created_at",
            ],
            vec![
                k.id.into(),
                k.application_id.into(),
                k.environment_id.into(),
                k.name.into(),
                k.key_hash.into(),
                k.key_prefix.into(),
                k.scopes.into(),
                k.expires_at.map(Into::into).unwrap_or(Value::BigInt(None)),
                k.last_used_at
                    .map(Into::into)
                    .unwrap_or(Value::BigInt(None)),
                k.revoked_at.map(Into::into).unwrap_or(Value::BigInt(None)),
                k.created_at.into(),
            ],
        )
        .await;
    }

    // 7. Events
    if !backup.events.is_empty() {
        let event_cols = &[
            "id",
            "application_id",
            "environment_id",
            "name",
            "timestamp",
            "day",
            "anonymous_id",
            "session_id",
            "app_version",
            "launcher_version",
            "os",
            "attributes",
            "dedupe_key",
            "received_at",
        ];
        let chunks: Vec<_> = backup.events.chunks(200).collect();
        for chunk in chunks {
            let rows: Vec<Vec<Value>> = chunk
                .iter()
                .map(|e| {
                    vec![
                        e.id.clone().into(),
                        e.application_id.clone().into(),
                        e.environment_id.clone().into(),
                        e.name.clone().into(),
                        e.timestamp.into(),
                        e.day.clone().into(),
                        e.anonymous_id
                            .clone()
                            .map(Into::into)
                            .unwrap_or(Value::String(None)),
                        e.session_id
                            .clone()
                            .map(Into::into)
                            .unwrap_or(Value::String(None)),
                        e.app_version
                            .clone()
                            .map(Into::into)
                            .unwrap_or(Value::String(None)),
                        e.launcher_version
                            .clone()
                            .map(Into::into)
                            .unwrap_or(Value::String(None)),
                        e.os.clone().map(Into::into).unwrap_or(Value::String(None)),
                        e.attributes.clone().into(),
                        e.dedupe_key
                            .clone()
                            .map(Into::into)
                            .unwrap_or(Value::String(None)),
                        e.received_at.into(),
                    ]
                })
                .collect();
            let _ = insert_batch(database, "events", event_cols, rows).await;
        }
    }

    // 8. Metric Points
    if !backup.metric_points.is_empty() {
        let mp_cols = &[
            "id",
            "application_id",
            "environment_id",
            "name",
            "metric_type",
            "value",
            "unit",
            "timestamp",
            "attributes",
            "received_at",
        ];
        let chunks: Vec<_> = backup.metric_points.chunks(200).collect();
        for chunk in chunks {
            let rows: Vec<Vec<Value>> = chunk
                .iter()
                .map(|mp| {
                    vec![
                        mp.id.clone().into(),
                        mp.application_id.clone().into(),
                        mp.environment_id.clone().into(),
                        mp.name.clone().into(),
                        mp.metric_type.clone().into(),
                        mp.value.into(),
                        mp.unit
                            .clone()
                            .map(Into::into)
                            .unwrap_or(Value::String(None)),
                        mp.timestamp.into(),
                        mp.attributes.clone().into(),
                        mp.received_at.into(),
                    ]
                })
                .collect();
            let _ = insert_batch(database, "metric_points", mp_cols, rows).await;
        }
    }

    // 9. Logs
    if !backup.logs.is_empty() {
        let log_cols = &[
            "id",
            "application_id",
            "environment_id",
            "level",
            "message",
            "logger",
            "trace_id",
            "span_id",
            "timestamp",
            "attributes",
            "received_at",
        ];
        let chunks: Vec<_> = backup.logs.chunks(200).collect();
        for chunk in chunks {
            let rows: Vec<Vec<Value>> = chunk
                .iter()
                .map(|l| {
                    vec![
                        l.id.clone().into(),
                        l.application_id.clone().into(),
                        l.environment_id.clone().into(),
                        l.level.clone().into(),
                        l.message.clone().into(),
                        l.logger
                            .clone()
                            .map(Into::into)
                            .unwrap_or(Value::String(None)),
                        l.trace_id
                            .clone()
                            .map(Into::into)
                            .unwrap_or(Value::String(None)),
                        l.span_id
                            .clone()
                            .map(Into::into)
                            .unwrap_or(Value::String(None)),
                        l.timestamp.into(),
                        l.attributes.clone().into(),
                        l.received_at.into(),
                    ]
                })
                .collect();
            let _ = insert_batch(database, "logs", log_cols, rows).await;
        }
    }

    // 10. Daily Aggregates
    if !backup.daily_aggregates.is_empty() {
        let da_cols = &[
            "id",
            "application_id",
            "environment_id",
            "day",
            "kind",
            "dimension",
            "dimension_value",
            "count",
            "sum",
            "updated_at",
        ];
        let chunks: Vec<_> = backup.daily_aggregates.chunks(200).collect();
        for chunk in chunks {
            let rows: Vec<Vec<Value>> = chunk
                .iter()
                .map(|da| {
                    vec![
                        da.id.clone().into(),
                        da.application_id.clone().into(),
                        da.environment_id.clone().into(),
                        da.day.clone().into(),
                        da.kind.clone().into(),
                        da.dimension.clone().into(),
                        da.dimension_value.clone().into(),
                        da.count.into(),
                        da.sum.map(Into::into).unwrap_or(Value::Double(None)),
                        da.updated_at.into(),
                    ]
                })
                .collect();
            let _ = insert_batch(database, "daily_aggregates", da_cols, rows).await;
        }
    }

    // 11. Alert Rules
    for ar in backup.alert_rules {
        let _ = insert(
            database,
            "alert_rules",
            &[
                "id",
                "application_id",
                "name",
                "enabled",
                "source_kind",
                "query_json",
                "window_minutes",
                "cooldown_seconds",
                "last_state",
                "last_evaluated_at",
                "created_at",
            ],
            vec![
                ar.id.into(),
                ar.application_id.into(),
                ar.name.into(),
                ar.enabled.into(),
                ar.source_kind.into(),
                ar.query_json.into(),
                ar.window_minutes.into(),
                ar.cooldown_seconds.into(),
                ar.last_state.into(),
                ar.last_evaluated_at
                    .map(Into::into)
                    .unwrap_or(Value::BigInt(None)),
                ar.created_at.into(),
            ],
        )
        .await;
    }

    // 12. Notification Channels
    for nc in backup.notification_channels {
        let _ = insert(
            database,
            "notification_channels",
            &["id", "name", "kind", "config_json", "enabled", "created_at"],
            vec![
                nc.id.into(),
                nc.name.into(),
                nc.kind.into(),
                nc.config_json.into(),
                nc.enabled.into(),
                nc.created_at.into(),
            ],
        )
        .await;
    }

    // 13. Audit Log
    for al in backup.audit_log {
        let _ = insert(
            database,
            "audit_log",
            &[
                "id",
                "actor_user_id",
                "action",
                "resource_type",
                "resource_id",
                "metadata",
                "created_at",
            ],
            vec![
                al.id.into(),
                al.actor_user_id
                    .map(Into::into)
                    .unwrap_or(Value::String(None)),
                al.action.into(),
                al.resource_type.into(),
                al.resource_id
                    .map(Into::into)
                    .unwrap_or(Value::String(None)),
                al.metadata.into(),
                al.created_at.into(),
            ],
        )
        .await;
    }

    Ok(())
}
