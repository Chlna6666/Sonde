use std::{collections::BTreeMap, path::Path};

use futures_util::Stream;
use sea_orm::{
    AccessMode, ConnectionTrait, DatabaseConnection, DbBackend, DbErr, IsolationLevel, QueryResult,
    TransactionTrait,
    sea_query::{Alias, Expr, ExprTrait, Order, Query, Value},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::{
    fs::File,
    io::{AsyncBufReadExt, BufReader},
};

use super::{
    backup_repo::{
        BackupAlertRule, BackupApiKey, BackupApplication, BackupAuditLog, BackupDailyAggregate,
        BackupEnvironment, BackupEvent, BackupLog, BackupNotificationChannel, BackupRole,
        BackupRoleBinding, BackupUser,
    },
    query::insert_batch_ignore_conflicts,
};

pub const FORMAT_VERSION: &str = "2.1";
const LEGACY_FORMAT_VERSION: &str = "2.0";
pub const BACKUP_TYPE: &str = "sonde_full_backup_ndjson";
pub const CONTENT_TYPE: &str = "application/x-ndjson";
pub const FILE_EXTENSION: &str = "sonde.ndjson";
const EXPORT_BATCH_ROWS: u64 = 512;
const RESTORE_BATCH_ROWS: usize = 256;
pub const MAX_RECORD_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupV2Manifest {
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
pub struct BackupV2End {
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
pub struct BackupMetricPointV2 {
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
pub enum BackupV2Record {
    Manifest(BackupV2Manifest),
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
    MetricPoint(BackupMetricPointV2),
    Log(BackupLog),
    ErrorGroup(BackupErrorGroup),
    ErrorOccurrence(BackupErrorOccurrence),
    DailyAggregate(BackupDailyAggregate),
    DailyRollup(BackupDailyRollup),
    AuditLog(BackupAuditLog),
    End(BackupV2End),
}

#[derive(Debug, thiserror::Error)]
pub enum BackupV2Error {
    #[error("backup I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("backup JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("backup database operation failed: {0}")]
    Database(#[from] DbErr),
    #[error("invalid backup: {0}")]
    Invalid(String),
}

type RecordMapper = fn(QueryResult) -> Result<BackupV2Record, DbErr>;

struct TableSpec {
    table: &'static str,
    columns: &'static [&'static str],
    mapper: RecordMapper,
}

pub fn export_full_system_stream(
    database: DatabaseConnection,
) -> impl Stream<Item = Result<Vec<u8>, BackupV2Error>> {
    async_stream::try_stream! {
        // Keep all cursor pages on one logical database snapshot. PostgreSQL defaults to
        // READ COMMITTED, so long-running backups explicitly request REPEATABLE READ. SQLite's
        // regular read transaction already pins its snapshot after the first read.
        let transaction = match database.get_database_backend() {
            DbBackend::Postgres | DbBackend::MySql => database
                .begin_with_config(Some(IsolationLevel::RepeatableRead), Some(AccessMode::ReadOnly))
                .await?,
            _ => database.begin().await?,
        };

        let manifest = BackupV2Record::Manifest(BackupV2Manifest {
            format_version: FORMAT_VERSION.to_owned(),
            backup_type: BACKUP_TYPE.to_owned(),
            exported_at: chrono::Utc::now().timestamp_millis(),
            server_version: env!("CARGO_PKG_VERSION").to_owned(),
            // Password hashes and notification-channel credentials are part of a faithful system
            // restore. Treat the resulting archive as a secret until field encryption is added.
            contains_secrets: true,
            // TOTP secrets are intentionally excluded in v2. Restoring onto a fresh database leaves
            // 2FA disabled and requires explicit re-enrollment instead of copying MFA seed material.
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

        let end = BackupV2Record::End(BackupV2End {
            records,
            sha256: hex::encode(digest.finalize()),
        });
        let end_line = encode_line(&end)?;
        transaction.commit().await?;
        yield end_line;
    }
}

pub async fn validate_backup_file(path: &Path) -> Result<BackupV2Manifest, BackupV2Error> {
    let file = File::open(path).await?;
    let mut reader = BufReader::new(file);
    let mut line = Vec::with_capacity(4096);
    let mut digest = Sha256::new();
    let mut manifest: Option<BackupV2Manifest> = None;
    let mut records = 0_u64;
    let mut saw_end = false;

    loop {
        line.clear();
        let read = reader.read_until(b'\n', &mut line).await?;
        if read == 0 {
            break;
        }
        if line.len() > MAX_RECORD_BYTES {
            return Err(BackupV2Error::Invalid(format!(
                "record exceeds {} bytes",
                MAX_RECORD_BYTES
            )));
        }
        let payload = record_payload(&line)?;
        let record: BackupV2Record = serde_json::from_slice(payload)?;

        if saw_end {
            return Err(BackupV2Error::Invalid("data found after end record".into()));
        }

        match record {
            BackupV2Record::Manifest(value) => {
                if manifest.is_some() || records != 0 {
                    return Err(BackupV2Error::Invalid(
                        "manifest must be the first and only manifest record".into(),
                    ));
                }
                validate_manifest(&value)?;
                manifest = Some(value);
                digest.update(&line);
            }
            BackupV2Record::End(end) => {
                if manifest.is_none() {
                    return Err(BackupV2Error::Invalid("missing manifest".into()));
                }
                if end.records != records {
                    return Err(BackupV2Error::Invalid(format!(
                        "record count mismatch: expected {}, got {}",
                        end.records, records
                    )));
                }
                let actual = hex::encode(digest.finalize_reset());
                if actual != end.sha256.to_ascii_lowercase() {
                    return Err(BackupV2Error::Invalid("backup SHA-256 mismatch".into()));
                }
                saw_end = true;
            }
            _ => {
                if manifest.is_none() {
                    return Err(BackupV2Error::Invalid("manifest must be first".into()));
                }
                records = records.saturating_add(1);
                digest.update(&line);
            }
        }
    }

    if !saw_end {
        return Err(BackupV2Error::Invalid("missing end record".into()));
    }
    manifest.ok_or_else(|| BackupV2Error::Invalid("missing manifest".into()))
}

pub async fn restore_full_system_from_file(
    database: &DatabaseConnection,
    path: &Path,
) -> Result<u64, BackupV2Error> {
    // Validation is deliberately a separate first pass. No database transaction or write lock is
    // held while the HTTP body is arriving or while a potentially corrupt archive is being checked.
    validate_backup_file(path).await?;

    let file = File::open(path).await?;
    let mut reader = BufReader::new(file);
    let transaction = database.begin().await?;
    let mut line = Vec::with_capacity(4096);
    let mut batch: Option<RestoreBatch> = None;
    let mut restored = 0_u64;

    loop {
        line.clear();
        let read = reader.read_until(b'\n', &mut line).await?;
        if read == 0 {
            break;
        }
        let record: BackupV2Record = serde_json::from_slice(record_payload(&line)?)?;
        let Some(insert_row) = record_to_insert(record) else {
            continue;
        };

        let needs_flush = batch.as_ref().is_some_and(|current| {
            current.table != insert_row.table
                || current.rows.len() >= RESTORE_BATCH_ROWS
        });
        if needs_flush {
            flush_batch(&transaction, batch.take()).await?;
        }

        let current = batch.get_or_insert_with(|| RestoreBatch {
            table: insert_row.table,
            columns: insert_row.columns,
            rows: Vec::with_capacity(RESTORE_BATCH_ROWS),
        });
        current.rows.push(insert_row.values);
        restored = restored.saturating_add(1);
    }

    flush_batch(&transaction, batch.take()).await?;
    transaction.commit().await?;
    Ok(restored)
}

fn validate_manifest(manifest: &BackupV2Manifest) -> Result<(), BackupV2Error> {
    if !matches!(manifest.format_version.as_str(), FORMAT_VERSION | LEGACY_FORMAT_VERSION) {
        return Err(BackupV2Error::Invalid(format!(
            "unsupported format version {}",
            manifest.format_version
        )));
    }
    if manifest.backup_type != BACKUP_TYPE {
        return Err(BackupV2Error::Invalid(format!(
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

fn encode_line(record: &BackupV2Record) -> Result<Vec<u8>, BackupV2Error> {
    let mut line = serde_json::to_vec(record)?;
    line.push(b'\n');
    if line.len() > MAX_RECORD_BYTES {
        return Err(BackupV2Error::Invalid(format!(
            "record exceeds {} bytes",
            MAX_RECORD_BYTES
        )));
    }
    Ok(line)
}

fn record_payload(line: &[u8]) -> Result<&[u8], BackupV2Error> {
    let mut end = line.len();
    if end > 0 && line[end - 1] == b'\n' {
        end -= 1;
    }
    if end > 0 && line[end - 1] == b'\r' {
        end -= 1;
    }
    if end == 0 {
        return Err(BackupV2Error::Invalid("blank NDJSON record".into()));
    }
    Ok(&line[..end])
}

struct InsertRow {
    table: &'static str,
    columns: &'static [&'static str],
    values: Vec<Value>,
}

struct RestoreBatch {
    table: &'static str,
    columns: &'static [&'static str],
    rows: Vec<Vec<Value>>,
}

async fn flush_batch(
    database: &impl ConnectionTrait,
    batch: Option<RestoreBatch>,
) -> Result<(), DbErr> {
    let Some(batch) = batch else {
        return Ok(());
    };
    insert_batch_ignore_conflicts(
        database,
        batch.table,
        batch.columns,
        batch.rows,
        "id",
        "id",
    )
    .await?;
    Ok(())
}

fn record_to_insert(record: BackupV2Record) -> Option<InsertRow> {
    match record {
        BackupV2Record::Manifest(_) | BackupV2Record::End(_) => None,
        BackupV2Record::Role(v) => Some(InsertRow {
            table: "roles",
            columns: &["id", "name", "builtin", "permissions", "created_at"],
            values: vec![v.id.into(), v.name.into(), v.builtin.into(), v.permissions.into(), v.created_at.into()],
        }),
        BackupV2Record::User(v) => Some(InsertRow {
            table: "users",
            columns: &["id", "email", "username", "password_hash", "locale", "active", "created_at"],
            values: vec![v.id.into(), v.email.into(), v.username.into(), v.password_hash.into(), v.locale.into(), v.active.into(), v.created_at.into()],
        }),
        BackupV2Record::Application(v) => Some(InsertRow {
            table: "applications",
            columns: &["id", "name", "slug", "retention_days", "owner_user_id", "is_public", "description", "github_url", "website_url", "custom_header", "created_at"],
            values: vec![
                v.id.into(), v.name.into(), v.slug.into(), v.retention_days.into(), Value::from(v.owner_user_id),
                v.is_public.into(), Value::from(v.description), Value::from(v.github_url), Value::from(v.website_url),
                Value::from(v.custom_header), v.created_at.into(),
            ],
        }),
        BackupV2Record::RoleBinding(v) => Some(InsertRow {
            table: "role_bindings",
            columns: &["id", "user_id", "role_id", "application_id", "created_at"],
            values: vec![v.id.into(), v.user_id.into(), v.role_id.into(), Value::from(v.application_id), v.created_at.into()],
        }),
        BackupV2Record::Environment(v) => Some(InsertRow {
            table: "environments",
            columns: &["id", "application_id", "name", "slug", "created_at"],
            values: vec![v.id.into(), v.application_id.into(), v.name.into(), v.slug.into(), v.created_at.into()],
        }),
        BackupV2Record::ApiKey(v) => Some(InsertRow {
            table: "api_keys",
            columns: &["id", "application_id", "environment_id", "name", "key_hash", "key_prefix", "scopes", "expires_at", "last_used_at", "revoked_at", "created_at"],
            values: vec![
                v.id.into(), v.application_id.into(), v.environment_id.into(), v.name.into(), v.key_hash.into(), v.key_prefix.into(),
                v.scopes.into(), Value::from(v.expires_at), Value::from(v.last_used_at), Value::from(v.revoked_at), v.created_at.into(),
            ],
        }),
        BackupV2Record::AlertRule(v) => Some(InsertRow {
            table: "alert_rules",
            columns: &["id", "application_id", "name", "enabled", "source_kind", "query_json", "window_minutes", "cooldown_seconds", "last_state", "last_evaluated_at", "created_at"],
            values: vec![
                v.id.into(), v.application_id.into(), v.name.into(), v.enabled.into(), v.source_kind.into(), v.query_json.into(),
                v.window_minutes.into(), v.cooldown_seconds.into(), v.last_state.into(), Value::from(v.last_evaluated_at), v.created_at.into(),
            ],
        }),
        BackupV2Record::NotificationChannel(v) => Some(InsertRow {
            table: "notification_channels",
            columns: &["id", "name", "kind", "config_json", "enabled", "created_at"],
            values: vec![v.id.into(), v.name.into(), v.kind.into(), v.config_json.into(), v.enabled.into(), v.created_at.into()],
        }),
        BackupV2Record::AlertDelivery(v) => Some(InsertRow {
            table: "alert_deliveries",
            columns: &["id", "rule_id", "channel_id", "status", "attempts", "last_error", "next_attempt_at", "created_at"],
            values: vec![v.id.into(), v.rule_id.into(), v.channel_id.into(), v.status.into(), v.attempts.into(), Value::from(v.last_error), Value::from(v.next_attempt_at), v.created_at.into()],
        }),
        BackupV2Record::ImportRun(v) => Some(InsertRow {
            table: "import_runs",
            columns: &["id", "source_type", "source_hash", "application_id", "environment_id", "status", "inserted", "deduped", "rejected", "created_at"],
            values: vec![v.id.into(), v.source_type.into(), v.source_hash.into(), v.application_id.into(), v.environment_id.into(), v.status.into(), v.inserted.into(), v.deduped.into(), v.rejected.into(), v.created_at.into()],
        }),
        BackupV2Record::Event(v) => Some(InsertRow {
            table: "events",
            columns: &["id", "application_id", "environment_id", "name", "timestamp", "day", "anonymous_id", "session_id", "app_version", "launcher_version", "os", "attributes", "dedupe_key", "received_at"],
            values: vec![
                v.id.into(), v.application_id.into(), v.environment_id.into(), v.name.into(), v.timestamp.into(), v.day.into(),
                Value::from(v.anonymous_id), Value::from(v.session_id), Value::from(v.app_version), Value::from(v.launcher_version),
                Value::from(v.os), v.attributes.into(), Value::from(v.dedupe_key), v.received_at.into(),
            ],
        }),
        BackupV2Record::MetricPoint(v) => Some(InsertRow {
            table: "metric_points",
            columns: &[
                "id", "application_id", "environment_id", "name", "metric_type", "value", "unit",
                "timestamp", "attributes", "received_at", "histogram_count", "histogram_sum",
                "histogram_min", "histogram_max", "histogram_bounds", "histogram_bucket_counts",
            ],
            values: vec![
                v.id.into(), v.application_id.into(), v.environment_id.into(), v.name.into(),
                v.metric_type.into(), v.value.into(), Value::from(v.unit), v.timestamp.into(),
                v.attributes.into(), v.received_at.into(), Value::from(v.histogram_count),
                Value::from(v.histogram_sum), Value::from(v.histogram_min), Value::from(v.histogram_max),
                Value::from(v.histogram_bounds), Value::from(v.histogram_bucket_counts),
            ],
        }),
        BackupV2Record::Log(v) => Some(InsertRow {
            table: "logs",
            columns: &["id", "application_id", "environment_id", "level", "message", "logger", "trace_id", "span_id", "timestamp", "attributes", "received_at"],
            values: vec![v.id.into(), v.application_id.into(), v.environment_id.into(), v.level.into(), v.message.into(), Value::from(v.logger), Value::from(v.trace_id), Value::from(v.span_id), v.timestamp.into(), v.attributes.into(), v.received_at.into()],
        }),
        BackupV2Record::ErrorGroup(v) => Some(InsertRow {
            table: "error_groups",
            columns: &["id", "application_id", "environment_id", "fingerprint", "name", "message_sample", "severity", "first_seen", "last_seen", "occurrences", "last_app_version", "last_launcher_version", "last_os", "updated_at"],
            values: vec![v.id.into(), v.application_id.into(), v.environment_id.into(), v.fingerprint.into(), v.name.into(), v.message_sample.into(), v.severity.into(), v.first_seen.into(), v.last_seen.into(), v.occurrences.into(), Value::from(v.last_app_version), Value::from(v.last_launcher_version), Value::from(v.last_os), v.updated_at.into()],
        }),
        BackupV2Record::ErrorOccurrence(v) => Some(InsertRow {
            table: "error_occurrences",
            columns: &["id", "group_id", "application_id", "environment_id", "timestamp", "anonymous_id", "session_id", "app_version", "launcher_version", "os", "stack_trace", "handled", "attributes", "received_at"],
            values: vec![v.id.into(), v.group_id.into(), v.application_id.into(), v.environment_id.into(), v.timestamp.into(), Value::from(v.anonymous_id), Value::from(v.session_id), Value::from(v.app_version), Value::from(v.launcher_version), Value::from(v.os), Value::from(v.stack_trace), Value::from(v.handled), v.attributes.into(), v.received_at.into()],
        }),
        BackupV2Record::DailyAggregate(v) => Some(InsertRow {
            table: "daily_aggregates",
            columns: &["id", "application_id", "environment_id", "day", "kind", "dimension", "dimension_value", "count", "sum", "updated_at"],
            values: vec![v.id.into(), v.application_id.into(), v.environment_id.into(), v.day.into(), v.kind.into(), v.dimension.into(), v.dimension_value.into(), v.count.into(), Value::from(v.sum), v.updated_at.into()],
        }),
        BackupV2Record::DailyRollup(v) => Some(InsertRow {
            table: "telemetry_daily_rollups",
            columns: &["id", "application_id", "environment_id", "day", "events", "users", "metrics", "logs", "errors", "updated_at"],
            values: vec![v.id.into(), v.application_id.into(), v.environment_id.into(), v.day.into(), v.events.into(), v.users.into(), v.metrics.into(), v.logs.into(), v.errors.into(), v.updated_at.into()],
        }),
        BackupV2Record::AuditLog(v) => Some(InsertRow {
            table: "audit_log",
            columns: &["id", "actor_user_id", "action", "resource_type", "resource_id", "metadata", "created_at"],
            values: vec![v.id.into(), Value::from(v.actor_user_id), v.action.into(), v.resource_type.into(), Value::from(v.resource_id), v.metadata.into(), v.created_at.into()],
        }),
    }
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

fn map_role(r: QueryResult) -> Result<BackupV2Record, DbErr> {
    Ok(BackupV2Record::Role(BackupRole { id: r.try_get("", "id")?, name: r.try_get("", "name")?, builtin: r.try_get("", "builtin")?, permissions: r.try_get("", "permissions")?, created_at: r.try_get("", "created_at")? }))
}
fn map_user(r: QueryResult) -> Result<BackupV2Record, DbErr> {
    Ok(BackupV2Record::User(BackupUser { id: r.try_get("", "id")?, email: r.try_get("", "email")?, username: r.try_get("", "username")?, password_hash: r.try_get("", "password_hash")?, locale: r.try_get("", "locale")?, active: r.try_get("", "active")?, created_at: r.try_get("", "created_at")? }))
}
fn map_application(r: QueryResult) -> Result<BackupV2Record, DbErr> {
    Ok(BackupV2Record::Application(BackupApplication { id: r.try_get("", "id")?, name: r.try_get("", "name")?, slug: r.try_get("", "slug")?, retention_days: r.try_get("", "retention_days")?, owner_user_id: r.try_get("", "owner_user_id").ok(), is_public: r.try_get("", "is_public").unwrap_or(false), description: r.try_get("", "description").ok(), github_url: r.try_get("", "github_url").ok(), website_url: r.try_get("", "website_url").ok(), custom_header: r.try_get("", "custom_header").ok(), created_at: r.try_get("", "created_at")? }))
}
fn map_role_binding(r: QueryResult) -> Result<BackupV2Record, DbErr> {
    Ok(BackupV2Record::RoleBinding(BackupRoleBinding { id: r.try_get("", "id")?, user_id: r.try_get("", "user_id")?, role_id: r.try_get("", "role_id")?, application_id: r.try_get("", "application_id").ok(), created_at: r.try_get("", "created_at")? }))
}
fn map_environment(r: QueryResult) -> Result<BackupV2Record, DbErr> {
    Ok(BackupV2Record::Environment(BackupEnvironment { id: r.try_get("", "id")?, application_id: r.try_get("", "application_id")?, name: r.try_get("", "name")?, slug: r.try_get("", "slug")?, created_at: r.try_get("", "created_at")? }))
}
fn map_api_key(r: QueryResult) -> Result<BackupV2Record, DbErr> {
    Ok(BackupV2Record::ApiKey(BackupApiKey { id: r.try_get("", "id")?, application_id: r.try_get("", "application_id")?, environment_id: r.try_get("", "environment_id")?, name: r.try_get("", "name")?, key_hash: r.try_get("", "key_hash")?, key_prefix: r.try_get("", "key_prefix")?, scopes: r.try_get("", "scopes")?, expires_at: r.try_get("", "expires_at").ok(), last_used_at: r.try_get("", "last_used_at").ok(), revoked_at: r.try_get("", "revoked_at").ok(), created_at: r.try_get("", "created_at")? }))
}
fn map_alert_rule(r: QueryResult) -> Result<BackupV2Record, DbErr> {
    Ok(BackupV2Record::AlertRule(BackupAlertRule { id: r.try_get("", "id")?, application_id: r.try_get("", "application_id")?, name: r.try_get("", "name")?, enabled: r.try_get("", "enabled")?, source_kind: r.try_get("", "source_kind")?, query_json: r.try_get("", "query_json")?, window_minutes: r.try_get("", "window_minutes")?, cooldown_seconds: r.try_get("", "cooldown_seconds")?, last_state: r.try_get("", "last_state")?, last_evaluated_at: r.try_get("", "last_evaluated_at").ok(), created_at: r.try_get("", "created_at")? }))
}
fn map_notification_channel(r: QueryResult) -> Result<BackupV2Record, DbErr> {
    Ok(BackupV2Record::NotificationChannel(BackupNotificationChannel { id: r.try_get("", "id")?, name: r.try_get("", "name")?, kind: r.try_get("", "kind")?, config_json: r.try_get("", "config_json")?, enabled: r.try_get("", "enabled")?, created_at: r.try_get("", "created_at")? }))
}
fn map_alert_delivery(r: QueryResult) -> Result<BackupV2Record, DbErr> {
    Ok(BackupV2Record::AlertDelivery(BackupAlertDelivery { id: r.try_get("", "id")?, rule_id: r.try_get("", "rule_id")?, channel_id: r.try_get("", "channel_id")?, status: r.try_get("", "status")?, attempts: r.try_get("", "attempts")?, last_error: r.try_get("", "last_error").ok(), next_attempt_at: r.try_get("", "next_attempt_at").ok(), created_at: r.try_get("", "created_at")? }))
}
fn map_import_run(r: QueryResult) -> Result<BackupV2Record, DbErr> {
    Ok(BackupV2Record::ImportRun(BackupImportRun { id: r.try_get("", "id")?, source_type: r.try_get("", "source_type")?, source_hash: r.try_get("", "source_hash")?, application_id: r.try_get("", "application_id")?, environment_id: r.try_get("", "environment_id")?, status: r.try_get("", "status")?, inserted: r.try_get("", "inserted")?, deduped: r.try_get("", "deduped")?, rejected: r.try_get("", "rejected")?, created_at: r.try_get("", "created_at")? }))
}
fn map_event(r: QueryResult) -> Result<BackupV2Record, DbErr> {
    Ok(BackupV2Record::Event(BackupEvent { id: r.try_get("", "id")?, application_id: r.try_get("", "application_id")?, environment_id: r.try_get("", "environment_id")?, name: r.try_get("", "name")?, timestamp: r.try_get("", "timestamp")?, day: r.try_get("", "day")?, anonymous_id: r.try_get("", "anonymous_id").ok(), session_id: r.try_get("", "session_id").ok(), app_version: r.try_get("", "app_version").ok(), launcher_version: r.try_get("", "launcher_version").ok(), os: r.try_get("", "os").ok(), attributes: r.try_get("", "attributes")?, dedupe_key: r.try_get("", "dedupe_key").ok(), received_at: r.try_get("", "received_at")? }))
}
fn map_metric_point(r: QueryResult) -> Result<BackupV2Record, DbErr> {
    Ok(BackupV2Record::MetricPoint(BackupMetricPointV2 {
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
fn map_log(r: QueryResult) -> Result<BackupV2Record, DbErr> {
    Ok(BackupV2Record::Log(BackupLog { id: r.try_get("", "id")?, application_id: r.try_get("", "application_id")?, environment_id: r.try_get("", "environment_id")?, level: r.try_get("", "level")?, message: r.try_get("", "message")?, logger: r.try_get("", "logger").ok(), trace_id: r.try_get("", "trace_id").ok(), span_id: r.try_get("", "span_id").ok(), timestamp: r.try_get("", "timestamp")?, attributes: r.try_get("", "attributes")?, received_at: r.try_get("", "received_at")? }))
}
fn map_error_group(r: QueryResult) -> Result<BackupV2Record, DbErr> {
    Ok(BackupV2Record::ErrorGroup(BackupErrorGroup { id: r.try_get("", "id")?, application_id: r.try_get("", "application_id")?, environment_id: r.try_get("", "environment_id")?, fingerprint: r.try_get("", "fingerprint")?, name: r.try_get("", "name")?, message_sample: r.try_get("", "message_sample")?, severity: r.try_get("", "severity")?, first_seen: r.try_get("", "first_seen")?, last_seen: r.try_get("", "last_seen")?, occurrences: r.try_get("", "occurrences")?, last_app_version: r.try_get("", "last_app_version").ok(), last_launcher_version: r.try_get("", "last_launcher_version").ok(), last_os: r.try_get("", "last_os").ok(), updated_at: r.try_get("", "updated_at")? }))
}
fn map_error_occurrence(r: QueryResult) -> Result<BackupV2Record, DbErr> {
    Ok(BackupV2Record::ErrorOccurrence(BackupErrorOccurrence { id: r.try_get("", "id")?, group_id: r.try_get("", "group_id")?, application_id: r.try_get("", "application_id")?, environment_id: r.try_get("", "environment_id")?, timestamp: r.try_get("", "timestamp")?, anonymous_id: r.try_get("", "anonymous_id").ok(), session_id: r.try_get("", "session_id").ok(), app_version: r.try_get("", "app_version").ok(), launcher_version: r.try_get("", "launcher_version").ok(), os: r.try_get("", "os").ok(), stack_trace: r.try_get("", "stack_trace").ok(), handled: r.try_get("", "handled").ok(), attributes: r.try_get("", "attributes")?, received_at: r.try_get("", "received_at")? }))
}
fn map_daily_aggregate(r: QueryResult) -> Result<BackupV2Record, DbErr> {
    Ok(BackupV2Record::DailyAggregate(BackupDailyAggregate { id: r.try_get("", "id")?, application_id: r.try_get("", "application_id")?, environment_id: r.try_get("", "environment_id")?, day: r.try_get("", "day")?, kind: r.try_get("", "kind")?, dimension: r.try_get("", "dimension")?, dimension_value: r.try_get("", "dimension_value")?, count: r.try_get("", "count")?, sum: r.try_get("", "sum").ok(), updated_at: r.try_get("", "updated_at")? }))
}
fn map_daily_rollup(r: QueryResult) -> Result<BackupV2Record, DbErr> {
    Ok(BackupV2Record::DailyRollup(BackupDailyRollup { id: r.try_get("", "id")?, application_id: r.try_get("", "application_id")?, environment_id: r.try_get("", "environment_id")?, day: r.try_get("", "day")?, events: r.try_get("", "events")?, users: r.try_get("", "users")?, metrics: r.try_get("", "metrics")?, logs: r.try_get("", "logs")?, errors: r.try_get("", "errors")?, updated_at: r.try_get("", "updated_at")? }))
}
fn map_audit_log(r: QueryResult) -> Result<BackupV2Record, DbErr> {
    Ok(BackupV2Record::AuditLog(BackupAuditLog { id: r.try_get("", "id")?, actor_user_id: r.try_get("", "actor_user_id").ok(), action: r.try_get("", "action")?, resource_type: r.try_get("", "resource_type")?, resource_id: r.try_get("", "resource_id").ok(), metadata: r.try_get("", "metadata")?, created_at: r.try_get("", "created_at")? }))
}

pub fn manifest_summary(manifest: &BackupV2Manifest) -> BTreeMap<&'static str, serde_json::Value> {
    BTreeMap::from([
        ("formatVersion", serde_json::Value::String(manifest.format_version.clone())),
        ("backupType", serde_json::Value::String(manifest.backup_type.clone())),
        ("exportedAt", serde_json::Value::from(manifest.exported_at)),
        ("containsSecrets", serde_json::Value::Bool(manifest.contains_secrets)),
    ])
}
