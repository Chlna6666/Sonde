use std::collections::HashMap;

use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr, TransactionTrait,
    sea_query::{Alias, Condition, Expr, ExprTrait, OnConflict, Order, Query, Value},
};

use super::{device_activity, telemetry::TelemetryScope};

const CURSOR_KEY: &str = "device_activity_backfill_cursor";
const COMPLETE_KEY: &str = "device_activity_backfill_complete";

#[derive(Clone, Debug)]
struct EventActivity {
    id: String,
    application_id: String,
    environment_id: String,
    device_hash: String,
    timestamp: i64,
}

#[derive(Clone, Debug)]
struct DeviceBounds {
    application_id: String,
    environment_id: String,
    device_hash: String,
    first_seen_at: i64,
    last_seen_at: i64,
}

pub async fn run_batch(database: &DatabaseConnection, limit: u64) -> Result<usize, DbErr> {
    if is_complete(database).await? {
        return Ok(0);
    }

    let cursor = read_cursor(database).await?;
    let mut query = Query::select();
    query
        .columns(
            [
                "id",
                "application_id",
                "environment_id",
                "anonymous_id",
                "timestamp",
            ]
            .map(Alias::new),
        )
        .from(Alias::new("events"))
        .and_where(Expr::col(Alias::new("anonymous_id")).is_not_null())
        .order_by(Alias::new("timestamp"), Order::Asc)
        .order_by(Alias::new("id"), Order::Asc)
        .limit(std::cmp::max(limit, 1));
    if let Some((timestamp, id)) = cursor.as_ref() {
        query.and_where(
            Condition::any()
                .add(Expr::col(Alias::new("timestamp")).gt(*timestamp))
                .add(
                    Condition::all()
                        .add(Expr::col(Alias::new("timestamp")).eq(*timestamp))
                        .add(Expr::col(Alias::new("id")).gt(id.as_str())),
                ),
        );
    }

    let raw_rows = database.query_all(&query.to_owned()).await?;
    let mut rows = Vec::with_capacity(raw_rows.len());
    for row in raw_rows {
        let Some(device_hash) = row.try_get::<Option<String>>("", "anonymous_id")? else {
            continue;
        };
        rows.push(EventActivity {
            id: row.try_get("", "id")?,
            application_id: row.try_get("", "application_id")?,
            environment_id: row.try_get("", "environment_id")?,
            device_hash,
            timestamp: row.try_get("", "timestamp")?,
        });
    }

    if rows.is_empty() {
        set_state(database, COMPLETE_KEY, "complete").await?;
        return Ok(0);
    }

    let mut bounds = HashMap::<(String, String, String), DeviceBounds>::new();
    for row in &rows {
        let key = (
            row.application_id.clone(),
            row.environment_id.clone(),
            row.device_hash.clone(),
        );
        bounds
            .entry(key)
            .and_modify(|bounds| {
                bounds.first_seen_at = std::cmp::min(bounds.first_seen_at, row.timestamp);
                bounds.last_seen_at = std::cmp::max(bounds.last_seen_at, row.timestamp);
            })
            .or_insert_with(|| DeviceBounds {
                application_id: row.application_id.clone(),
                environment_id: row.environment_id.clone(),
                device_hash: row.device_hash.clone(),
                first_seen_at: row.timestamp,
                last_seen_at: row.timestamp,
            });
    }

    let transaction = database.begin().await?;
    for bounds in bounds.values() {
        merge_device_bounds(&transaction, bounds).await?;
    }

    let mut previous = HashMap::<(String, String, String), i64>::new();
    for row in &rows {
        let key = (
            row.application_id.clone(),
            row.environment_id.clone(),
            row.device_hash.clone(),
        );
        let previous_seen_at = previous.insert(key, row.timestamp).unwrap_or(row.timestamp);
        device_activity::record(
            &transaction,
            &TelemetryScope {
                application_id: row.application_id.clone(),
                environment_id: row.environment_id.clone(),
            },
            &row.device_hash,
            previous_seen_at,
            row.timestamp,
        )
        .await?;
    }

    let last = rows
        .last()
        .ok_or_else(|| DbErr::Custom("device activity backfill batch unexpectedly empty".into()))?;
    set_state(
        &transaction,
        CURSOR_KEY,
        &format!("{}:{}", last.timestamp, last.id),
    )
    .await?;
    transaction.commit().await?;
    Ok(rows.len())
}

pub async fn is_complete(database: &impl ConnectionTrait) -> Result<bool, DbErr> {
    Ok(read_state(database, COMPLETE_KEY)
        .await?
        .is_some_and(|value| value == "complete"))
}

pub async fn invalidate(database: &impl ConnectionTrait) -> Result<(), DbErr> {
    for key in [CURSOR_KEY, COMPLETE_KEY] {
        let delete = Query::delete()
            .from_table(Alias::new("system_state"))
            .and_where(Expr::col(Alias::new("key")).eq(key))
            .to_owned();
        database.execute(&delete).await?;
    }
    Ok(())
}

async fn merge_device_bounds(
    database: &impl ConnectionTrait,
    bounds: &DeviceBounds,
) -> Result<(), DbErr> {
    let now = chrono::Utc::now().timestamp_millis();
    let mut insert = Query::insert();
    insert.into_table(Alias::new("telemetry_devices")).columns(
        [
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
        ]
        .map(Alias::new),
    );
    let values = [
        Value::from(bounds.device_hash.clone()),
        Value::from(bounds.application_id.clone()),
        Value::from(bounds.environment_id.clone()),
        Value::from(bounds.device_hash.clone()),
        Value::from(bounds.first_seen_at),
        Value::from(bounds.last_seen_at),
        Value::from(Some(bounds.last_seen_at)),
        Value::BigInt(None),
        Value::BigInt(None),
        Value::BigInt(None),
        Value::String(None),
        Value::BigInt(None),
        Value::String(None),
        Value::BigInt(None),
        Value::String(None),
        Value::BigInt(None),
        Value::String(None),
        Value::BigInt(None),
        Value::String(None),
        Value::BigInt(None),
        Value::String(None),
        Value::BigInt(None),
        Value::from(0_i64),
        Value::from(0_i64),
        Value::from(0_i64),
        Value::from(0_i64),
        Value::from(0_i64),
        Value::from(0_i64),
        Value::from(0_i64),
        Value::from(0_i64),
        Value::from(0_i32),
        Value::String(None),
        Value::BigInt(None),
        Value::from(now),
    ];
    insert
        .values(values.into_iter().map(Expr::value))
        .map_err(|error| DbErr::Custom(format!("build device backfill insert: {error}")))?;
    insert.on_conflict(
        OnConflict::column(Alias::new("id"))
            .do_nothing()
            .to_owned(),
    );
    database.execute(&insert).await?;

    let update = Query::update()
        .table(Alias::new("telemetry_devices"))
        .value(
            Alias::new("first_seen_at"),
            Expr::cust(format!(
                "CASE WHEN first_seen_at IS NULL OR first_seen_at > {} THEN {} ELSE first_seen_at END",
                bounds.first_seen_at, bounds.first_seen_at
            )),
        )
        .value(
            Alias::new("last_seen_at"),
            Expr::cust(format!(
                "CASE WHEN last_seen_at < {} THEN {} ELSE last_seen_at END",
                bounds.last_seen_at, bounds.last_seen_at
            )),
        )
        .value(
            Alias::new("last_event_at"),
            Expr::cust(format!(
                "CASE WHEN last_event_at IS NULL OR last_event_at < {} THEN {} ELSE last_event_at END",
                bounds.last_seen_at, bounds.last_seen_at
            )),
        )
        .and_where(Expr::col(Alias::new("id")).eq(&bounds.device_hash))
        .to_owned();
    database.execute(&update).await?;
    Ok(())
}

async fn read_cursor(database: &impl ConnectionTrait) -> Result<Option<(i64, String)>, DbErr> {
    let Some(value) = read_state(database, CURSOR_KEY).await? else {
        return Ok(None);
    };
    let Some((timestamp, id)) = value.split_once(':') else {
        return Err(DbErr::Custom("invalid device activity backfill cursor".into()));
    };
    let timestamp = timestamp
        .parse::<i64>()
        .map_err(|_| DbErr::Custom("invalid device activity backfill timestamp".into()))?;
    Ok(Some((timestamp, id.to_owned())))
}

async fn read_state(
    database: &impl ConnectionTrait,
    key: &str,
) -> Result<Option<String>, DbErr> {
    let query = Query::select()
        .column(Alias::new("value"))
        .from(Alias::new("system_state"))
        .and_where(Expr::col(Alias::new("key")).eq(key))
        .limit(1)
        .to_owned();
    database
        .query_one(&query)
        .await?
        .map(|row| row.try_get::<String>("", "value"))
        .transpose()
}

async fn set_state(
    database: &impl ConnectionTrait,
    key: &str,
    value: &str,
) -> Result<(), DbErr> {
    let mut query = Query::insert();
    query
        .into_table(Alias::new("system_state"))
        .columns([Alias::new("key"), Alias::new("value")]);
    let values = [Value::from(key), Value::from(value)];
    query
        .values(values.into_iter().map(Expr::value))
        .map_err(|error| DbErr::Custom(format!("build system state upsert: {error}")))?;
    query.on_conflict(
        OnConflict::column(Alias::new("key"))
            .update_column(Alias::new("value"))
            .to_owned(),
    );
    database.execute(&query).await?;
    Ok(())
}
