use std::path::Path;

use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr, TransactionTrait,
    sea_query::{Alias, Expr, ExprTrait, Query, Value},
};
use tokio::{
    fs::File,
    io::{AsyncBufReadExt, BufReader},
};

use crate::database::{
    backup_archive::{self, BackupError, BackupRecord},
    query::insert_batch_ignore_conflicts,
};

const RESTORE_BATCH_ROWS: usize = 256;
const DERIVED_STATE_KEYS: &[&str] = &[
    "telemetry_rollup_backfill",
    "telemetry_dimension_rollup_backfill",
    "telemetry_user_rollup_backfill",
    "telemetry_log_error_rollup_backfill",
    "telemetry_first_seen_backfill",
    "telemetry_first_seen_backfill_cursor",
];

pub async fn restore_full_system_exact(
    database: &DatabaseConnection,
    path: &Path,
) -> Result<u64, BackupError> {
    // Pass 1 validates the complete archive before any destructive database operation happens.
    backup_archive::validate_backup_file(path).await?;

    let transaction = database.begin().await?;
    clear_restorable_state(&transaction).await?;

    let file = File::open(path).await?;
    let mut reader = BufReader::new(file);
    let mut line = Vec::with_capacity(4096);
    let mut batch: Option<RestoreBatch> = None;
    let mut restored = 0_u64;

    loop {
        line.clear();
        let read = reader.read_until(b'\n', &mut line).await?;
        if read == 0 {
            break;
        }
        if line.len() > backup_archive::MAX_RECORD_BYTES {
            return Err(BackupError::Invalid(format!(
                "record exceeds {} bytes",
                backup_archive::MAX_RECORD_BYTES
            )));
        }
        let record: BackupRecord = serde_json::from_slice(record_payload(&line)?)?;
        let Some(row) = into_insert_row(record) else {
            continue;
        };

        let needs_flush = batch.as_ref().is_some_and(|current| {
            current.table != row.table || current.rows.len() >= RESTORE_BATCH_ROWS
        });
        if needs_flush {
            flush_batch(&transaction, batch.take()).await?;
        }

        let current = batch.get_or_insert_with(|| RestoreBatch {
            table: row.table,
            columns: row.columns,
            rows: Vec::with_capacity(RESTORE_BATCH_ROWS),
        });
        current.rows.push(row.values);
        restored = restored.saturating_add(1);
    }

    flush_batch(&transaction, batch.take()).await?;
    transaction.commit().await?;
    Ok(restored)
}

async fn clear_restorable_state(database: &impl ConnectionTrait) -> Result<(), DbErr> {
    // Rebuildable telemetry projections are cleared in the same destructive transaction as the
    // authoritative restore. Their readiness/cursor keys are invalidated before commit, so a reader
    // can never observe restored raw telemetry together with stale derived statistics.
    for table in [
        "auth_totp_replay",
        "auth_2fa_pending",
        "auth_sessions",
        "job_leases",
        "telemetry_dirty_days",
        "telemetry_daily_dimensions",
        "telemetry_daily_user_sets",
        "telemetry_daily_log_errors",
        "alert_deliveries",
        "error_occurrences",
        "error_groups",
        "daily_aggregates",
        "telemetry_daily_rollups",
        "logs",
        "metric_points",
        "events",
        "api_keys",
        "environments",
        "role_bindings",
        "alert_rules",
        "notification_channels",
        "import_runs",
        "audit_log",
        "applications",
        "users",
        "roles",
    ] {
        let delete = Query::delete().from_table(Alias::new(table)).to_owned();
        database.execute(&delete).await?;
    }

    let invalidate = Query::delete()
        .from_table(Alias::new("system_state"))
        .and_where(Expr::col(Alias::new("key")).is_in(DERIVED_STATE_KEYS.iter().copied()))
        .to_owned();
    database.execute(&invalidate).await?;
    Ok(())
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
    // The target tables were cleared in this transaction. Keeping conflict handling here also makes
    // a malformed archive with duplicate ids deterministic rather than aborting halfway through.
    insert_batch_ignore_conflicts(database, batch.table, batch.columns, batch.rows, "id", "id")
        .await?;
    Ok(())
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

fn into_insert_row(record: BackupRecord) -> Option<InsertRow> {
    match record {
        BackupRecord::Manifest(_) | BackupRecord::End(_) => None,
        BackupRecord::Role(value) => Some(InsertRow {
            table: "roles",
            columns: &["id", "name", "builtin", "permissions", "created_at"],
            values: vec![
                value.id.into(),
                value.name.into(),
                value.builtin.into(),
                value.permissions.into(),
                value.created_at.into(),
            ],
        }),
        BackupRecord::User(value) => Some(InsertRow {
            table: "users",
            columns: &[
                "id",
                "email",
                "username",
                "password_hash",
                "locale",
                "active",
                "created_at",
            ],
            values: vec![
                value.id.into(),
                value.email.into(),
                value.username.into(),
                value.password_hash.into(),
                value.locale.into(),
                value.active.into(),
                value.created_at.into(),
            ],
        }),
        BackupRecord::Application(value) => Some(InsertRow {
            table: "applications",
            columns: &[
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
            values: vec![
                value.id.into(),
                value.name.into(),
                value.slug.into(),
                value.retention_days.into(),
                Value::from(value.owner_user_id),
                value.is_public.into(),
                Value::from(value.description),
                Value::from(value.github_url),
                Value::from(value.website_url),
                Value::from(value.custom_header),
                value.created_at.into(),
            ],
        }),
        BackupRecord::RoleBinding(value) => Some(InsertRow {
            table: "role_bindings",
            columns: &["id", "user_id", "role_id", "application_id", "created_at"],
            values: vec![
                value.id.into(),
                value.user_id.into(),
                value.role_id.into(),
                Value::from(value.application_id),
                value.created_at.into(),
            ],
        }),
        BackupRecord::Environment(value) => Some(InsertRow {
            table: "environments",
            columns: &["id", "application_id", "name", "slug", "created_at"],
            values: vec![
                value.id.into(),
                value.application_id.into(),
                value.name.into(),
                value.slug.into(),
                value.created_at.into(),
            ],
        }),
        BackupRecord::ApiKey(value) => Some(InsertRow {
            table: "api_keys",
            columns: &[
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
            values: vec![
                value.id.into(),
                value.application_id.into(),
                value.environment_id.into(),
                value.name.into(),
                value.key_hash.into(),
                value.key_prefix.into(),
                value.scopes.into(),
                Value::from(value.expires_at),
                Value::from(value.last_used_at),
                Value::from(value.revoked_at),
                value.created_at.into(),
            ],
        }),
        BackupRecord::AlertRule(value) => Some(InsertRow {
            table: "alert_rules",
            columns: &[
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
            values: vec![
                value.id.into(),
                value.application_id.into(),
                value.name.into(),
                value.enabled.into(),
                value.source_kind.into(),
                value.query_json.into(),
                value.window_minutes.into(),
                value.cooldown_seconds.into(),
                value.last_state.into(),
                Value::from(value.last_evaluated_at),
                value.created_at.into(),
            ],
        }),
        BackupRecord::NotificationChannel(value) => Some(InsertRow {
            table: "notification_channels",
            columns: &["id", "name", "kind", "config_json", "enabled", "created_at"],
            values: vec![
                value.id.into(),
                value.name.into(),
                value.kind.into(),
                value.config_json.into(),
                value.enabled.into(),
                value.created_at.into(),
            ],
        }),
        BackupRecord::AlertDelivery(value) => Some(InsertRow {
            table: "alert_deliveries",
            columns: &[
                "id",
                "rule_id",
                "channel_id",
                "status",
                "attempts",
                "last_error",
                "next_attempt_at",
                "created_at",
            ],
            values: vec![
                value.id.into(),
                value.rule_id.into(),
                value.channel_id.into(),
                value.status.into(),
                value.attempts.into(),
                Value::from(value.last_error),
                Value::from(value.next_attempt_at),
                value.created_at.into(),
            ],
        }),
        BackupRecord::ImportRun(value) => Some(InsertRow {
            table: "import_runs",
            columns: &[
                "id",
                "source_type",
                "source_hash",
                "application_id",
                "environment_id",
                "status",
                "inserted",
                "deduped",
                "rejected",
                "created_at",
            ],
            values: vec![
                value.id.into(),
                value.source_type.into(),
                value.source_hash.into(),
                value.application_id.into(),
                value.environment_id.into(),
                value.status.into(),
                value.inserted.into(),
                value.deduped.into(),
                value.rejected.into(),
                value.created_at.into(),
            ],
        }),
        BackupRecord::Event(value) => Some(InsertRow {
            table: "events",
            columns: &[
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
            values: vec![
                value.id.into(),
                value.application_id.into(),
                value.environment_id.into(),
                value.name.into(),
                value.timestamp.into(),
                value.day.into(),
                Value::from(value.anonymous_id),
                Value::from(value.session_id),
                Value::from(value.app_version),
                Value::from(value.launcher_version),
                Value::from(value.os),
                value.attributes.into(),
                Value::from(value.dedupe_key),
                value.received_at.into(),
            ],
        }),
        BackupRecord::MetricPoint(value) => Some(InsertRow {
            table: "metric_points",
            columns: &[
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
                "histogram_count",
                "histogram_sum",
                "histogram_min",
                "histogram_max",
                "histogram_bounds",
                "histogram_bucket_counts",
            ],
            values: vec![
                value.id.into(),
                value.application_id.into(),
                value.environment_id.into(),
                value.name.into(),
                value.metric_type.into(),
                value.value.into(),
                Value::from(value.unit),
                value.timestamp.into(),
                value.attributes.into(),
                value.received_at.into(),
                Value::from(value.histogram_count),
                Value::from(value.histogram_sum),
                Value::from(value.histogram_min),
                Value::from(value.histogram_max),
                Value::from(value.histogram_bounds),
                Value::from(value.histogram_bucket_counts),
            ],
        }),
        BackupRecord::Log(value) => Some(InsertRow {
            table: "logs",
            columns: &[
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
            values: vec![
                value.id.into(),
                value.application_id.into(),
                value.environment_id.into(),
                value.level.into(),
                value.message.into(),
                Value::from(value.logger),
                Value::from(value.trace_id),
                Value::from(value.span_id),
                value.timestamp.into(),
                value.attributes.into(),
                value.received_at.into(),
            ],
        }),
        BackupRecord::ErrorGroup(value) => Some(InsertRow {
            table: "error_groups",
            columns: &[
                "id",
                "application_id",
                "environment_id",
                "fingerprint",
                "name",
                "message_sample",
                "severity",
                "first_seen",
                "last_seen",
                "occurrences",
                "last_app_version",
                "last_launcher_version",
                "last_os",
                "updated_at",
            ],
            values: vec![
                value.id.into(),
                value.application_id.into(),
                value.environment_id.into(),
                value.fingerprint.into(),
                value.name.into(),
                value.message_sample.into(),
                value.severity.into(),
                value.first_seen.into(),
                value.last_seen.into(),
                value.occurrences.into(),
                Value::from(value.last_app_version),
                Value::from(value.last_launcher_version),
                Value::from(value.last_os),
                value.updated_at.into(),
            ],
        }),
        BackupRecord::ErrorOccurrence(value) => Some(InsertRow {
            table: "error_occurrences",
            columns: &[
                "id",
                "group_id",
                "application_id",
                "environment_id",
                "timestamp",
                "anonymous_id",
                "session_id",
                "app_version",
                "launcher_version",
                "os",
                "stack_trace",
                "handled",
                "attributes",
                "received_at",
            ],
            values: vec![
                value.id.into(),
                value.group_id.into(),
                value.application_id.into(),
                value.environment_id.into(),
                value.timestamp.into(),
                Value::from(value.anonymous_id),
                Value::from(value.session_id),
                Value::from(value.app_version),
                Value::from(value.launcher_version),
                Value::from(value.os),
                Value::from(value.stack_trace),
                Value::from(value.handled),
                value.attributes.into(),
                value.received_at.into(),
            ],
        }),
        BackupRecord::DailyAggregate(value) => Some(InsertRow {
            table: "daily_aggregates",
            columns: &[
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
            ],
            values: vec![
                value.id.into(),
                value.application_id.into(),
                value.environment_id.into(),
                value.day.into(),
                value.kind.into(),
                value.dimension.into(),
                value.dimension_value.into(),
                value.count.into(),
                Value::from(value.sum),
                value.updated_at.into(),
            ],
        }),
        BackupRecord::DailyRollup(value) => Some(InsertRow {
            table: "telemetry_daily_rollups",
            columns: &[
                "id",
                "application_id",
                "environment_id",
                "day",
                "events",
                "users",
                "metrics",
                "logs",
                "errors",
                "updated_at",
            ],
            values: vec![
                value.id.into(),
                value.application_id.into(),
                value.environment_id.into(),
                value.day.into(),
                value.events.into(),
                value.users.into(),
                value.metrics.into(),
                value.logs.into(),
                value.errors.into(),
                value.updated_at.into(),
            ],
        }),
        BackupRecord::AuditLog(value) => Some(InsertRow {
            table: "audit_log",
            columns: &[
                "id",
                "actor_user_id",
                "action",
                "resource_type",
                "resource_id",
                "metadata",
                "created_at",
            ],
            values: vec![
                value.id.into(),
                Value::from(value.actor_user_id),
                value.action.into(),
                value.resource_type.into(),
                Value::from(value.resource_id),
                value.metadata.into(),
                value.created_at.into(),
            ],
        }),
    }
}
