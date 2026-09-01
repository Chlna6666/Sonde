use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr, TransactionTrait,
    sea_query::{Alias, Condition, Expr, ExprTrait, Query},
};
use sha2::{Digest, Sha256};

use super::{query::insert_batch_ignore_conflicts, telemetry::TelemetryScope};

/// Project a historical imported observation into server-owned device identity indexes.
///
/// This path is intentionally idempotent at the device/bucket level. Historical imports do not
/// fabricate sessions, online duration or abuse signals; those require authoritative live receive
/// timing. Replaying or de-duplicating an import therefore cannot inflate activity duration.
pub async fn project_event(
    database: &DatabaseConnection,
    scope: &TelemetryScope,
    device_hash: &str,
    timestamp: i64,
) -> Result<(), DbErr> {
    let transaction = database.begin().await?;
    ensure_device(&transaction, scope, device_hash, timestamp).await?;
    merge_device_bounds(&transaction, device_hash, timestamp).await?;
    ensure_activity_bucket(
        &transaction,
        "telemetry_device_activity_days",
        "day",
        "sonde:device-activity-day\0",
        scope,
        device_hash,
        &day_for_timestamp(timestamp),
        timestamp,
    )
    .await?;
    ensure_activity_bucket(
        &transaction,
        "telemetry_device_activity_hours",
        "hour",
        "sonde:device-activity-hour\0",
        scope,
        device_hash,
        &hour_for_timestamp(timestamp),
        timestamp,
    )
    .await?;
    transaction.commit().await
}

async fn ensure_device(
    database: &impl ConnectionTrait,
    scope: &TelemetryScope,
    device_hash: &str,
    timestamp: i64,
) -> Result<(), DbErr> {
    insert_batch_ignore_conflicts(
        database,
        "telemetry_devices",
        &[
            "id",
            "application_id",
            "environment_id",
            "device_hash",
            "first_seen_at",
            "last_seen_at",
            "last_event_at",
            "last_metric_at",
            "last_log_at",
            "last_error_at",
            "last_session_id",
            "last_session_at",
            "last_app_version",
            "last_app_version_at",
            "last_launcher_version",
            "last_launcher_version_at",
            "last_os",
            "last_os_at",
            "last_system_language",
            "last_system_language_at",
            "last_architecture",
            "last_architecture_at",
            "event_items",
            "metric_items",
            "log_items",
            "error_items",
            "session_changes",
            "app_version_changes",
            "launcher_version_changes",
            "os_changes",
            "risk_score",
            "last_anomaly",
            "last_anomaly_at",
            "updated_at",
        ],
        vec![vec![
            device_hash.to_owned().into(),
            scope.application_id.clone().into(),
            scope.environment_id.clone().into(),
            device_hash.to_owned().into(),
            timestamp.into(),
            timestamp.into(),
            Some(timestamp).into(),
            Option::<i64>::None.into(),
            Option::<i64>::None.into(),
            Option::<i64>::None.into(),
            Option::<String>::None.into(),
            Option::<i64>::None.into(),
            Option::<String>::None.into(),
            Option::<i64>::None.into(),
            Option::<String>::None.into(),
            Option::<i64>::None.into(),
            Option::<String>::None.into(),
            Option::<i64>::None.into(),
            Option::<String>::None.into(),
            Option::<i64>::None.into(),
            Option::<String>::None.into(),
            Option::<i64>::None.into(),
            0_i64.into(),
            0_i64.into(),
            0_i64.into(),
            0_i64.into(),
            0_i64.into(),
            0_i64.into(),
            0_i64.into(),
            0_i64.into(),
            0_i32.into(),
            Option::<String>::None.into(),
            Option::<i64>::None.into(),
            timestamp.into(),
        ]],
        "id",
        "id",
    )
    .await?;
    Ok(())
}

async fn merge_device_bounds(
    database: &impl ConnectionTrait,
    device_hash: &str,
    timestamp: i64,
) -> Result<(), DbErr> {
    let earlier = Query::update()
        .table(Alias::new("telemetry_devices"))
        .value(Alias::new("first_seen_at"), timestamp)
        .and_where(Expr::col(Alias::new("id")).eq(device_hash))
        .cond_where(
            Condition::any()
                .add(Expr::col(Alias::new("first_seen_at")).is_null())
                .add(Expr::col(Alias::new("first_seen_at")).gt(timestamp)),
        )
        .to_owned();
    database.execute(&earlier).await?;

    let later = Query::update()
        .table(Alias::new("telemetry_devices"))
        .value(Alias::new("last_seen_at"), timestamp)
        .value(Alias::new("last_event_at"), timestamp)
        .value(Alias::new("updated_at"), timestamp)
        .and_where(Expr::col(Alias::new("id")).eq(device_hash))
        .and_where(Expr::col(Alias::new("last_seen_at")).lt(timestamp))
        .to_owned();
    database.execute(&later).await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn ensure_activity_bucket(
    database: &impl ConnectionTrait,
    table: &str,
    bucket_column: &str,
    context: &str,
    scope: &TelemetryScope,
    device_hash: &str,
    bucket: &str,
    timestamp: i64,
) -> Result<(), DbErr> {
    let id = activity_id(
        context,
        &scope.application_id,
        &scope.environment_id,
        device_hash,
        bucket,
    );
    insert_batch_ignore_conflicts(
        database,
        table,
        &[
            "id",
            "application_id",
            "environment_id",
            "device_hash",
            bucket_column,
            "first_seen_at",
            "last_seen_at",
            "active_millis",
            "request_count",
            "updated_at",
        ],
        vec![vec![
            id.clone().into(),
            scope.application_id.clone().into(),
            scope.environment_id.clone().into(),
            device_hash.to_owned().into(),
            bucket.to_owned().into(),
            timestamp.into(),
            timestamp.into(),
            0_i64.into(),
            1_i64.into(),
            timestamp.into(),
        ]],
        "id",
        "id",
    )
    .await?;

    let earlier = Query::update()
        .table(Alias::new(table))
        .value(Alias::new("first_seen_at"), timestamp)
        .and_where(Expr::col(Alias::new("id")).eq(&id))
        .and_where(Expr::col(Alias::new("first_seen_at")).gt(timestamp))
        .to_owned();
    database.execute(&earlier).await?;
    let later = Query::update()
        .table(Alias::new(table))
        .value(Alias::new("last_seen_at"), timestamp)
        .value(Alias::new("updated_at"), timestamp)
        .and_where(Expr::col(Alias::new("id")).eq(id))
        .and_where(Expr::col(Alias::new("last_seen_at")).lt(timestamp))
        .to_owned();
    database.execute(&later).await?;
    Ok(())
}

fn day_for_timestamp(timestamp: i64) -> String {
    chrono::DateTime::from_timestamp_millis(timestamp)
        .map(|value| value.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "1970-01-01".into())
}

fn hour_for_timestamp(timestamp: i64) -> String {
    chrono::DateTime::from_timestamp_millis(timestamp)
        .map(|value| value.format("%Y-%m-%d %H:00").to_string())
        .unwrap_or_else(|| "1970-01-01 00:00".into())
}

fn activity_id(
    context: &str,
    application_id: &str,
    environment_id: &str,
    device_hash: &str,
    bucket: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(context.as_bytes());
    for value in [application_id, environment_id, device_hash, bucket] {
        hasher.update(value.as_bytes());
        hasher.update(b"\0");
    }
    hex::encode(hasher.finalize())
}
