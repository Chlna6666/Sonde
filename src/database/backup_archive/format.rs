use std::{collections::BTreeMap, path::Path};

use futures_util::Stream;
use sea_orm::{
    AccessMode, ConnectionTrait, DatabaseConnection, DbBackend, DbErr, IsolationLevel, QueryResult,
    TransactionTrait,
    sea_query::{Alias, Expr, ExprTrait, Order, Query},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::{
    fs::File,
    io::{AsyncBufReadExt, BufReader},
};

use crate::database::backup_records::{
    BackupAlertRule, BackupApiKey, BackupApplication, BackupAuditLog, BackupDailyAggregate,
    BackupEnvironment, BackupEvent, BackupLog, BackupNotificationChannel, BackupRole,
    BackupRoleBinding, BackupUser,
};

pub const FORMAT_VERSION: &str = "2.1";
pub const BACKUP_TYPE: &str = "sonde_full_backup_ndjson";
pub const CONTENT_TYPE: &str = "application/x-ndjson";
pub const FILE_EXTENSION: &str = "sonde.ndjson";
const EXPORT_BATCH_ROWS: u64 = 512;
pub const MAX_RECORD_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupManifest {
    pub format_version: String,
    pub backup_type: String,
    pub exported_at: i64,
    pub server_version: String,
    pub contains_secrets: bool,
    pub totp_secrets_included: bool,
    pub ephemeral_auth_state_included: bool,
    pub generated_rollups_included: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupEnd {
    pub records: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupAlertDelivery {
    pub id: String,
    pub rule_id: String,
    pub channel_id: String,
    pub status: String,
    pub attempts: i32,
    pub last_error: Option<String>,
    pub next_attempt_at: Option<i64>,
    pub created_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupImportRun {
    pub id: String,
    pub source_type: String,
    pub source_hash: String,
    pub application_id: String,
    pub environment_id: String,
    pub status: String,
    pub inserted: i64,
    pub deduped: i64,
    pub rejected: i64,
    pub created_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupErrorGroup {
    pub id: String,
    pub application_id: String,
    pub environment_id: String,
    pub fingerprint: String,
    pub name: String,
    pub message_sample: String,
    pub severity: String,
    pub first_seen: i64,
    pub last_seen: i64,
    pub occurrences: i64,
    pub last_app_version: Option<String>,
    pub last_launcher_version: Option<String>,
    pub last_os: Option<String>,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupErrorOccurrence {
    pub id: String,
    pub group_id: String,
    pub application_id: String,
    pub environment_id: String,
    pub timestamp: i64,
    pub anonymous_id: Option<String>,
    pub session_id: Option<String>,
    pub app_version: Option<String>,
    pub launcher_version: Option<String>,
    pub os: Option<String>,
    pub stack_trace: Option<String>,
    pub handled: Option<i64>,
    pub attributes: String,
    pub received_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub histogram_count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub histogram_sum: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub histogram_min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub histogram_max: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub histogram_bounds: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub histogram_bucket_counts: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupDailyRollup {
    pub id: String,
    pub application_id: String,
    pub environment_id: String,
    pub day: String,
    pub events: i64,
    pub users: i64,
    pub metrics: i64,
    pub logs: i64,
    pub errors: i64,
    pub updated_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum BackupRecord {
    Manifest(BackupManifest),
    Role(BackupRole),
    User(BackupUser),
    Application(BackupApplication),
    RoleBinding(BackupRoleBinding),
    Environment(BackupEnvironment),
    ApiKey(BackupApiKey),
    AlertRule(BackupAlertRule),
    NotificationChannel(BackupNotificationChannel),
    AlertDelivery(BackupAlertDelivery),
    ImportRun(BackupImportRun),
    Event(BackupEvent),
    MetricPoint(BackupMetricPoint),
    Log(BackupLog),
    ErrorGroup(BackupErrorGroup),
    ErrorOccurrence(BackupErrorOccurrence),
    DailyAggregate(BackupDailyAggregate),
    DailyRollup(BackupDailyRollup),
    AuditLog(BackupAuditLog),
    End(BackupEnd),
}

#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error("backup I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("backup JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("backup database operation failed: {0}")]
    Database(#[from] DbErr),
    #[error("invalid backup: {0}")]
    Invalid(String),
}

type RecordMapper = fn(QueryResult) -> Result<BackupRecord, DbErr>;

struct TableSpec {
    table: &'static str,
    columns: &'static [&'static str],
    mapper: RecordMapper,
}

pub fn export_full_system_stream(
    database: DatabaseConnection,
) -> impl Stream<Item = Result<Vec<u8>, BackupError>> {
    async_stream::try_stream! {
        let transaction = match database.get_database_backend() {
            DbBackend::Postgres | DbBackend::MySql => database
                .begin_with_config(Some(IsolationLevel::RepeatableRead), Some(AccessMode::ReadOnly))
                .await?,
            _ => database.begin().await?,
        };

        let manifest = BackupRecord::Manifest(BackupManifest {
            format_version: FORMAT_VERSION.to_owned(),
            backup_type: BACKUP_TYPE.to_owned(),
            exported_at: chrono::Utc::now().timestamp_millis(),
            server_version: env!("CARGO_PKG_VERSION").to_owned(),
            contains_secrets: true,
            totp_secrets_included: false,
            ephemeral_auth_state_included: false,
            generated_rollups_included: true,
        });

        let mut digest = Sha256::new();
        let manifest_line = encode_line(&manifest)?;
        digest.update(&manifest_line);
        yield manifest_line;

        let mut records = 0_u64;
        for spec in table_specs() {
            let mut after_id: Option<String> = None;
            loop {
                let rows = fetch_batch(
                    &transaction,
                    spec.table,
                    spec.columns,
                    after_id.as_deref(),
                )
                .await?;
                if rows.is_empty() {
                    break;
                }

                for row in rows {
                    let id: String = row.try_get("", "id")?;
                    let record = (spec.mapper)(row)?;
                    let line = encode_line(&record)?;
                    digest.update(&line);
                    records = records.saturating_add(1);
                    after_id = Some(id);
                    yield line;
                }
            }
        }

        let end = BackupRecord::End(BackupEnd {
            records,
            sha256: hex::encode(digest.finalize()),
        });
        let end_line = encode_line(&end)?;
        transaction.commit().await?;
        yield end_line;
    }
}

pub async fn validate_backup_file(path: &Path) -> Result<BackupManifest, BackupError> {
    let file = File::open(path).await?;
    let mut reader = BufReader::new(file);
    let mut line = Vec::with_capacity(4096);
    let mut digest = Sha256::new();
    let mut manifest: Option<BackupManifest> = None;
    let mut records = 0_u64;
    let mut saw_end = false;

    loop {
        line.clear();
        let read = reader.read_until(b'\n', &mut line).await?;
        if read == 0 {
            break;
        }
        if line.len() > MAX_RECORD_BYTES {
            return Err(BackupError::Invalid(format!(
                "record exceeds {} bytes",
                MAX_RECORD_BYTES
            )));
        }
        let payload = record_payload(&line)?;
        let record: BackupRecord = serde_json::from_slice(payload)?;

        if saw_end {
            return Err(BackupError::Invalid("data found after end record".into()));
        }

        match record {
            BackupRecord::Manifest(value) => {
                if manifest.is_some() || records != 0 {
                    return Err(BackupError::Invalid(
                        "manifest must be the first and only manifest record".into(),
                    ));
                }
                validate_manifest(&value)?;
                manifest = Some(value);
                digest.update(&line);
            }
            BackupRecord::End(end) => {
                if manifest.is_none() {
                    return Err(BackupError::Invalid("missing manifest".into()));
                }
                if end.records != records {
                    return Err(BackupError::Invalid(format!(
                        "record count mismatch: expected {}, got {}",
                        end.records, records
                    )));
                }
                let actual = hex::encode(digest.finalize_reset());
                if actual != end.sha256.to_ascii_lowercase() {
                    return Err(BackupError::Invalid("backup SHA-256 mismatch".into()));
                }
                saw_end = true;
            }
            _ => {
                if manifest.is_none() {
                    return Err(BackupError::Invalid("manifest must be first".into()));
                }
                records = records.saturating_add(1);
                digest.update(&line);
            }
        }
    }

    if !saw_end {
        return Err(BackupError::Invalid("missing end record".into()));
    }
    manifest.ok_or_else(|| BackupError::Invalid("missing manifest".into()))
}

fn validate_manifest(manifest: &BackupManifest) -> Result<(), BackupError> {
    if manifest.format_version != FORMAT_VERSION {
        return Err(BackupError::Invalid(format!(
            "unsupported format version {}",
            manifest.format_version
        )));
    }
    if manifest.backup_type != BACKUP_TYPE {
        return Err(BackupError::Invalid(format!(
            "unsupported backup type {}",
            manifest.backup_type
        )));
    }
    Ok(())
}

async fn fetch_batch(
    database: &impl ConnectionTrait,
    table: &str,
    columns: &[&str],
    after_id: Option<&str>,
) -> Result<Vec<QueryResult>, DbErr> {
    let mut query = Query::select();
    query
        .columns(columns.iter().map(|column| Alias::new(*column)))
        .from(Alias::new(table))
        .order_by(Alias::new("id"), Order::Asc)
        .limit(EXPORT_BATCH_ROWS);
    if let Some(after_id) = after_id {
        query.and_where(Expr::col(Alias::new("id")).gt(after_id));
    }
    database.query_all(&query).await
}

fn encode_line(record: &BackupRecord) -> Result<Vec<u8>, BackupError> {
    let mut line = serde_json::to_vec(record)?;
    line.push(b'\n');
    if line.len() > MAX_RECORD_BYTES {
        return Err(BackupError::Invalid(format!(
            "record exceeds {} bytes",
            MAX_RECORD_BYTES
        )));
    }
    Ok(line)
}

fn record_payload(line: &[u8]) -> Result<&[u8], BackupError> {
    let mut end = line.len();
    if end > 0 && line[end - 1] == b'\n' {
        end -= 1;
    }
    if end > 0 && line[end - 1] == b'\r' {
        end -= 1;
    }
    if end == 0 {
        return Err(BackupError::Invalid("blank NDJSON record".into()));
    }
    Ok(&line[..end])
}

fn table_specs() -> Vec<TableSpec> {
    vec![
        TableSpec { table: "roles", columns: &["id", "name", "builtin", "permissions", "created_at"], mapper: map_role },
        TableSpec { table: "users", columns: &["id", "email", "username", "password_hash", "locale", "active", "created_at"], mapper: map_user },
        TableSpec { table: "applications", columns: &["id", "name", "slug", "retention_days", "owner_user_id", "is_public", "description", "github_url", "website_url", "custom_header", "created_at"], mapper: map_application },
        TableSpec { table: "role_bindings", columns: &["id", "user_id", "role_id", "application_id", "created_at"], mapper: map_role_binding },
        TableSpec { table: "environments", columns: &["id", "application_id", "name", "slug", "created_at"], mapper: map_environment },
        TableSpec { table: "api_keys", columns: &["id", "application_id", "environment_id", "name", "key_hash", "key_prefix", "scopes", "expires_at", "last_used_at", "revoked_at", "created_at"], mapper: map_api_key },
        TableSpec { table: "alert_rules", columns: &["id", "application_id", "name", "enabled", "source_kind", "query_json", "window_minutes", "cooldown_seconds", "last_state", "last_evaluated_at", "created_at"], mapper: map_alert_rule },
        TableSpec { table: "notification_channels", columns: &["id", "name", "kind", "config_json", "enabled", "created_at"], mapper: map_notification_channel },
        TableSpec { table: "alert_deliveries", columns: &["id", "rule_id", "channel_id", "status", "attempts", "last_error", "next_attempt_at", "created_at"], mapper: map_alert_delivery },
        TableSpec { table: "import_runs", columns: &["id", "source_type", "source_hash", "application_id", "environment_id", "status", "inserted", "deduped", "rejected", "created_at"], mapper: map_import_run },
        TableSpec { table: "events", columns: &["id", "application_id", "environment_id", "name", "timestamp", "day", "anonymous_id", "session_id", "app_version", "launcher_version", "os", "attributes", "dedupe_key", "received_at"], mapper: map_event },
        TableSpec {
            table: "metric_points",
            columns: &[
                "id", "application_id", "environment_id", "name", "metric_type", "value", "unit",
                "timestamp", "attributes", "received_at", "histogram_count", "histogram_sum",
                "histogram_min", "histogram_max", "histogram_bounds", "histogram_bucket_counts",
            ],
            mapper: map_metric_point,
        },
        TableSpec { table: "logs", columns: &["id", "application_id", "environment_id", "level", "message", "logger", "trace_id", "span_id", "timestamp", "attributes", "received_at"], mapper: map_log },
        TableSpec { table: "error_groups", columns: &["id", "application_id", "environment_id", "fingerprint", "name", "message_sample", "severity", "first_seen", "last_seen", "occurrences", "last_app_version", "last_launcher_version", "last_os", "updated_at"], mapper: map_error_group },
        TableSpec { table: "error_occurrences", columns: &["id", "group_id", "application_id", "environment_id", "timestamp", "anonymous_id", "session_id", "app_version", "launcher_version", "os", "stack_trace", "handled", "attributes", "received_at"], mapper: map_error_occurrence },
        TableSpec { table: "daily_aggregates", columns: &["id", "application_id", "environment_id", "day", "kind", "dimension", "dimension_value", "count", "sum", "updated_at"], mapper: map_daily_aggregate },
        TableSpec { table: "telemetry_daily_rollups", columns: &["id", "application_id", "environment_id", "day", "events", "users", "metrics", "logs", "errors", "updated_at"], mapper: map_daily_rollup },
        TableSpec { table: "audit_log", columns: &["id", "actor_user_id", "action", "resource_type", "resource_id", "metadata", "created_at"], mapper: map_audit_log },
    ]
}

fn map_role(r: QueryResult) -> Result<BackupRecord, DbErr> {
    Ok(BackupRecord::Role(BackupRole { id: r.try_get("", "id")?, name: r.try_get("", "name")?, builtin: r.try_get("", "builtin")?, permissions: r.try_get("", "permissions")?, created_at: r.try_get("", "created_at")? }))
}
fn map_user(r: QueryResult) -> Result<BackupRecord, DbErr> {
    Ok(BackupRecord::User(BackupUser { id: r.try_get("", "id")?, email: r.try_get("", "email")?, username: r.try_get("", "username")?, password_hash: r.try_get("", "password_hash")?, locale: r.try_get("", "locale")?, active: r.try_get("", "active")?, created_at: r.try_get("", "created_at")? }))
}
fn map_application(r: QueryResult) -> Result<BackupRecord, DbErr> {
    Ok(BackupRecord::Application(BackupApplication { id: r.try_get("", "id")?, name: r.try_get("", "name")?, slug: r.try_get("", "slug")?, retention_days: r.try_get("", "retention_days")?, owner_user_id: r.try_get("", "owner_user_id").ok(), is_public: r.try_get("", "is_public").unwrap_or(false), description: r.try_get("", "description").ok(), github_url: r.try_get("", "github_url").ok(), website_url: r.try_get("", "website_url").ok(), custom_header: r.try_get("", "custom_header").ok(), created_at: r.try_get("", "created_at")? }))
}
fn map_role_binding(r: QueryResult) -> Result<BackupRecord, DbErr> {
    Ok(BackupRecord::RoleBinding(BackupRoleBinding { id: r.try_get("", "id")?, user_id: r.try_get("", "user_id")?, role_id: r.try_get("", "role_id")?, application_id: r.try_get("", "application_id").ok(), created_at: r.try_get("", "created_at")? }))
}
fn map_environment(r: QueryResult) -> Result<BackupRecord, DbErr> {
    Ok(BackupRecord::Environment(BackupEnvironment { id: r.try_get("", "id")?, application_id: r.try_get("", "application_id")?, name: r.try_get("", "name")?, slug: r.try_get("", "slug")?, created_at: r.try_get("", "created_at")? }))
}
fn map_api_key(r: QueryResult) -> Result<BackupRecord, DbErr> {
    Ok(BackupRecord::ApiKey(BackupApiKey { id: r.try_get("", "id")?, application_id: r.try_get("", "application_id")?, environment_id: r.try_get("", "environment_id")?, name: r.try_get("", "name")?, key_hash: r.try_get("", "key_hash")?, key_prefix: r.try_get("", "key_prefix")?, scopes: r.try_get("", "scopes")?, expires_at: r.try_get("", "expires_at").ok(), last_used_at: r.try_get("", "last_used_at").ok(), revoked_at: r.try_get("", "revoked_at").ok(), created_at: r.try_get("", "created_at")? }))
}
fn map_alert_rule(r: QueryResult) -> Result<BackupRecord, DbErr> {
    Ok(BackupRecord::AlertRule(BackupAlertRule { id: r.try_get("", "id")?, application_id: r.try_get("", "application_id")?, name: r.try_get("", "name")?, enabled: r.try_get("", "enabled")?, source_kind: r.try_get("", "source_kind")?, query_json: r.try_get("", "query_json")?, window_minutes: r.try_get("", "window_minutes")?, cooldown_seconds: r.try_get("", "cooldown_seconds")?, last_state: r.try_get("", "last_state")?, last_evaluated_at: r.try_get("", "last_evaluated_at").ok(), created_at: r.try_get("", "created_at")? }))
}
fn map_notification_channel(r: QueryResult) -> Result<BackupRecord, DbErr> {
    Ok(BackupRecord::NotificationChannel(BackupNotificationChannel { id: r.try_get("", "id")?, name: r.try_get("", "name")?, kind: r.try_get("", "kind")?, config_json: r.try_get("", "config_json")?, enabled: r.try_get("", "enabled")?, created_at: r.try_get("", "created_at")? }))
}
fn map_alert_delivery(r: QueryResult) -> Result<BackupRecord, DbErr> {
    Ok(BackupRecord::AlertDelivery(BackupAlertDelivery { id: r.try_get("", "id")?, rule_id: r.try_get("", "rule_id")?, channel_id: r.try_get("", "channel_id")?, status: r.try_get("", "status")?, attempts: r.try_get("", "attempts")?, last_error: r.try_get("", "last_error").ok(), next_attempt_at: r.try_get("", "next_attempt_at").ok(), created_at: r.try_get("", "created_at")? }))
}
fn map_import_run(r: QueryResult) -> Result<BackupRecord, DbErr> {
    Ok(BackupRecord::ImportRun(BackupImportRun { id: r.try_get("", "id")?, source_type: r.try_get("", "source_type")?, source_hash: r.try_get("", "source_hash")?, application_id: r.try_get("", "application_id")?, environment_id: r.try_get("", "environment_id")?, status: r.try_get("", "status")?, inserted: r.try_get("", "inserted")?, deduped: r.try_get("", "deduped")?, rejected: r.try_get("", "rejected")?, created_at: r.try_get("", "created_at")? }))
}
fn map_event(r: QueryResult) -> Result<BackupRecord, DbErr> {
    Ok(BackupRecord::Event(BackupEvent { id: r.try_get("", "id")?, application_id: r.try_get("", "application_id")?, environment_id: r.try_get("", "environment_id")?, name: r.try_get("", "name")?, timestamp: r.try_get("", "timestamp")?, day: r.try_get("", "day")?, anonymous_id: r.try_get("", "anonymous_id").ok(), session_id: r.try_get("", "session_id").ok(), app_version: r.try_get("", "app_version").ok(), launcher_version: r.try_get("", "launcher_version").ok(), os: r.try_get("", "os").ok(), attributes: r.try_get("", "attributes")?, dedupe_key: r.try_get("", "dedupe_key").ok(), received_at: r.try_get("", "received_at")? }))
}
fn map_metric_point(r: QueryResult) -> Result<BackupRecord, DbErr> {
    Ok(BackupRecord::MetricPoint(BackupMetricPoint {
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
        histogram_count: r.try_get("", "histogram_count").ok(),
        histogram_sum: r.try_get("", "histogram_sum").ok(),
        histogram_min: r.try_get("", "histogram_min").ok(),
        histogram_max: r.try_get("", "histogram_max").ok(),
        histogram_bounds: r.try_get("", "histogram_bounds").ok(),
        histogram_bucket_counts: r.try_get("", "histogram_bucket_counts").ok(),
    }))
}
fn map_log(r: QueryResult) -> Result<BackupRecord, DbErr> {
    Ok(BackupRecord::Log(BackupLog { id: r.try_get("", "id")?, application_id: r.try_get("", "application_id")?, environment_id: r.try_get("", "environment_id")?, level: r.try_get("", "level")?, message: r.try_get("", "message")?, logger: r.try_get("", "logger").ok(), trace_id: r.try_get("", "trace_id").ok(), span_id: r.try_get("", "span_id").ok(), timestamp: r.try_get("", "timestamp")?, attributes: r.try_get("", "attributes")?, received_at: r.try_get("", "received_at")? }))
}
fn map_error_group(r: QueryResult) -> Result<BackupRecord, DbErr> {
    Ok(BackupRecord::ErrorGroup(BackupErrorGroup { id: r.try_get("", "id")?, application_id: r.try_get("", "application_id")?, environment_id: r.try_get("", "environment_id")?, fingerprint: r.try_get("", "fingerprint")?, name: r.try_get("", "name")?, message_sample: r.try_get("", "message_sample")?, severity: r.try_get("", "severity")?, first_seen: r.try_get("", "first_seen")?, last_seen: r.try_get("", "last_seen")?, occurrences: r.try_get("", "occurrences")?, last_app_version: r.try_get("", "last_app_version").ok(), last_launcher_version: r.try_get("", "last_launcher_version").ok(), last_os: r.try_get("", "last_os").ok(), updated_at: r.try_get("", "updated_at")? }))
}
fn map_error_occurrence(r: QueryResult) -> Result<BackupRecord, DbErr> {
    Ok(BackupRecord::ErrorOccurrence(BackupErrorOccurrence { id: r.try_get("", "id")?, group_id: r.try_get("", "group_id")?, application_id: r.try_get("", "application_id")?, environment_id: r.try_get("", "environment_id")?, timestamp: r.try_get("", "timestamp")?, anonymous_id: r.try_get("", "anonymous_id").ok(), session_id: r.try_get("", "session_id").ok(), app_version: r.try_get("", "app_version").ok(), launcher_version: r.try_get("", "launcher_version").ok(), os: r.try_get("", "os").ok(), stack_trace: r.try_get("", "stack_trace").ok(), handled: r.try_get("", "handled").ok(), attributes: r.try_get("", "attributes")?, received_at: r.try_get("", "received_at")? }))
}
fn map_daily_aggregate(r: QueryResult) -> Result<BackupRecord, DbErr> {
    Ok(BackupRecord::DailyAggregate(BackupDailyAggregate { id: r.try_get("", "id")?, application_id: r.try_get("", "application_id")?, environment_id: r.try_get("", "environment_id")?, day: r.try_get("", "day")?, kind: r.try_get("", "kind")?, dimension: r.try_get("", "dimension")?, dimension_value: r.try_get("", "dimension_value")?, count: r.try_get("", "count")?, sum: r.try_get("", "sum").ok(), updated_at: r.try_get("", "updated_at")? }))
}
fn map_daily_rollup(r: QueryResult) -> Result<BackupRecord, DbErr> {
    Ok(BackupRecord::DailyRollup(BackupDailyRollup { id: r.try_get("", "id")?, application_id: r.try_get("", "application_id")?, environment_id: r.try_get("", "environment_id")?, day: r.try_get("", "day")?, events: r.try_get("", "events")?, users: r.try_get("", "users")?, metrics: r.try_get("", "metrics")?, logs: r.try_get("", "logs")?, errors: r.try_get("", "errors")?, updated_at: r.try_get("", "updated_at")? }))
}
fn map_audit_log(r: QueryResult) -> Result<BackupRecord, DbErr> {
    Ok(BackupRecord::AuditLog(BackupAuditLog { id: r.try_get("", "id")?, actor_user_id: r.try_get("", "actor_user_id").ok(), action: r.try_get("", "action")?, resource_type: r.try_get("", "resource_type")?, resource_id: r.try_get("", "resource_id").ok(), metadata: r.try_get("", "metadata")?, created_at: r.try_get("", "created_at")? }))
}

pub fn manifest_summary(manifest: &BackupManifest) -> BTreeMap<&'static str, serde_json::Value> {
    BTreeMap::from([
        ("formatVersion", serde_json::Value::String(manifest.format_version.clone())),
        ("backupType", serde_json::Value::String(manifest.backup_type.clone())),
        ("exportedAt", serde_json::Value::from(manifest.exported_at)),
        ("containsSecrets", serde_json::Value::Bool(manifest.contains_secrets)),
    ])
}
