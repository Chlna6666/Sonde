use std::collections::{BTreeMap, BTreeSet};

use chrono::NaiveDate;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, DbErr, TransactionTrait,
    sea_query::{Alias, Expr, ExprTrait, Func, OnConflict, Order, Query, SimpleExpr, Value},
};
use sha2::{Digest, Sha256};

use super::telemetry_repo::TelemetryScope;

const GLOBAL_ENVIRONMENT: &str = "*";
const ROLLUP_BACKFILL_KEY: &str = "telemetry_rollup_backfill_v1";
const DIRTY_INSERT_CHUNK: usize = 100;

pub const DIRTY_SOURCE_EVENT: i64 = 1;
pub const DIRTY_SOURCE_METRIC: i64 = 1 << 1;
pub const DIRTY_SOURCE_LOG: i64 = 1 << 2;
pub const DIRTY_SOURCE_ERROR: i64 = 1 << 3;
pub const DIRTY_SOURCE_LOG_ERROR: i64 = 1 << 4;
/// Sources materialized in `telemetry_daily_rollups` itself.
pub const DIRTY_SOURCE_ALL: i64 =
    DIRTY_SOURCE_EVENT | DIRTY_SOURCE_METRIC | DIRTY_SOURCE_LOG | DIRTY_SOURCE_ERROR;
/// All source bits accepted by the shared dirty-day queue, including independent derived domains.
pub const DIRTY_SOURCE_VALID: i64 = DIRTY_SOURCE_ALL | DIRTY_SOURCE_LOG_ERROR;

#[derive(Clone, Debug)]
pub struct DirtyDay {
    pub id: String,
    pub application_id: String,
    pub environment_id: String,
    pub day: String,
    pub marked_at: i64,
    pub generation: i64,
    pub source_mask: i64,
}

impl DirtyDay {
    pub fn has_source(&self, source: i64) -> bool {
        self.source_mask & source != 0
    }
}

#[derive(Clone, Debug)]
pub struct DailyRollupPoint {
    pub day: String,
    pub events: u64,
    pub users: u64,
    pub metrics: u64,
    pub logs: u64,
    pub errors: u64,
}

/// Conservative compatibility entrypoint used by lifecycle code. Ingestion should call
/// `mark_dirty_timestamps_for_source` so unrelated data sources do not invalidate event/user caches.
pub async fn mark_dirty_timestamps<I>(
    database: &impl ConnectionTrait,
    scope: &TelemetryScope,
    timestamps: I,
) -> Result<(), DbErr>
where
    I: IntoIterator<Item = i64>,
{
    mark_dirty_timestamps_for_source(database, scope, DIRTY_SOURCE_VALID, timestamps).await
}

pub async fn mark_dirty_timestamps_for_source<I>(
    database: &impl ConnectionTrait,
    scope: &TelemetryScope,
    source_mask: i64,
    timestamps: I,
) -> Result<(), DbErr>
where
    I: IntoIterator<Item = i64>,
{
    if source_mask <= 0 || source_mask & !DIRTY_SOURCE_VALID != 0 {
        return Err(DbErr::Custom("invalid telemetry dirty source mask".into()));
    }
    let days: BTreeSet<String> = timestamps.into_iter().filter_map(day_for_timestamp).collect();
    mark_dirty_days(database, scope, source_mask, days).await
}

async fn mark_dirty_days<I>(
    database: &impl ConnectionTrait,
    scope: &TelemetryScope,
    source_mask: i64,
    days: I,
) -> Result<(), DbErr>
where
    I: IntoIterator<Item = String>,
{
    let now = chrono::Utc::now().timestamp_millis();
    let mut rows = Vec::new();
    for day in days {
        for environment_id in [scope.environment_id.as_str(), GLOBAL_ENVIRONMENT] {
            rows.push(dirty_row(
                &scope.application_id,
                environment_id,
                &day,
                now,
                source_mask,
            ));
        }
    }
    upsert_dirty_rows(database, rows, source_mask).await
}

async fn upsert_dirty_rows(
    database: &impl ConnectionTrait,
    rows: Vec<Vec<Value>>,
    source_mask: i64,
) -> Result<(), DbErr> {
    for chunk in rows.chunks(DIRTY_INSERT_CHUNK) {
        let mut query = Query::insert();
        query
            .into_table(Alias::new("telemetry_dirty_days"))
            .columns(
                [
                    "id",
                    "application_id",
                    "environment_id",
                    "day",
                    "marked_at",
                    "generation",
                    "source_mask",
                ]
                .map(Alias::new),
            );
        for row in chunk {
            query
                .values(row.iter().cloned().map(Expr::value))
                .map_err(|error| DbErr::Custom(error.to_string()))?;
        }
        query.on_conflict(
            OnConflict::column(Alias::new("id"))
                .update_column(Alias::new("marked_at"))
                .values([
                    (
                        Alias::new("generation"),
                        Expr::col(Alias::new("generation")).add(1_i64),
                    ),
                    (
                        Alias::new("source_mask"),
                        Expr::cust(format!("source_mask | {source_mask}")),
                    ),
                ])
                .to_owned(),
        );
        database.execute(&query).await?;
    }
    Ok(())
}

pub fn dirty_source_condition(source_mask: i64) -> SimpleExpr {
    Expr::cust(format!("(source_mask & {source_mask}) <> 0"))
}

pub async fn list_dirty_days(
    database: &DatabaseConnection,
    limit: u64,
    marked_before: i64,
) -> Result<Vec<DirtyDay>, DbErr> {
    let query = Query::select()
        .columns(
            [
                "id",
                "application_id",
                "environment_id",
                "day",
                "marked_at",
                "generation",
                "source_mask",
            ]
            .map(Alias::new),
        )
        .from(Alias::new("telemetry_dirty_days"))
        .and_where(Expr::col(Alias::new("marked_at")).lte(marked_before))
        .order_by(Alias::new("marked_at"), Order::Asc)
        .limit(limit)
        .to_owned();
    let rows = database.query_all(&query).await?;
    rows.into_iter()
        .map(|row| {
            Ok(DirtyDay {
                id: row.try_get("", "id")?,
                application_id: row.try_get("", "application_id")?,
                environment_id: row.try_get("", "environment_id")?,
                day: row.try_get("", "day")?,
                marked_at: row.try_get("", "marked_at")?,
                generation: row.try_get("", "generation")?,
                source_mask: row.try_get("", "source_mask")?,
            })
        })
        .collect()
}

pub async fn recompute_claimed_day(
    database: &DatabaseConnection,
    dirty: DirtyDay,
) -> Result<bool, DbErr> {
    let transaction = database.begin().await?;

    let marker = Query::select()
        .columns(["generation", "source_mask"].map(Alias::new))
        .from(Alias::new("telemetry_dirty_days"))
        .and_where(Expr::col(Alias::new("id")).eq(&dirty.id))
        .limit(1)
        .to_owned();
    let current = transaction.query_one(&marker).await?;
    let generation = current
        .as_ref()
        .and_then(|row| row.try_get::<i64>("", "generation").ok());
    let source_mask = current
        .as_ref()
        .and_then(|row| row.try_get::<i64>("", "source_mask").ok());
    if generation != Some(dirty.generation) || source_mask != Some(dirty.source_mask) {
        transaction.rollback().await?;
        return Ok(false);
    }

    let (start, end) = day_bounds(&dirty.day)?;
    let environment = (dirty.environment_id != GLOBAL_ENVIRONMENT)
        .then_some(dirty.environment_id.as_str());
    let mut counts = if dirty.source_mask == DIRTY_SOURCE_ALL {
        (0, 0, 0, 0, 0)
    } else {
        existing_rollup_counts(
            &transaction,
            &dirty.application_id,
            &dirty.environment_id,
            &dirty.day,
        )
        .await?
    };

    if dirty.has_source(DIRTY_SOURCE_EVENT) {
        (counts.0, counts.1) = event_counts(
            &transaction,
            &dirty.application_id,
            environment,
            &dirty.day,
        )
        .await?;
    }
    if dirty.has_source(DIRTY_SOURCE_METRIC) {
        counts.2 = time_count(
            &transaction,
            "metric_points",
            &dirty.application_id,
            environment,
            start,
            end,
        )
        .await?;
    }
    if dirty.has_source(DIRTY_SOURCE_LOG) {
        counts.3 = time_count(
            &transaction,
            "logs",
            &dirty.application_id,
            environment,
            start,
            end,
        )
        .await?;
    }
    if dirty.has_source(DIRTY_SOURCE_ERROR) {
        counts.4 = time_count(
            &transaction,
            "error_occurrences",
            &dirty.application_id,
            environment,
            start,
            end,
        )
        .await?;
    }

    upsert_rollup(
        &transaction,
        &dirty.application_id,
        &dirty.environment_id,
        &dirty.day,
        counts.0,
        counts.1,
        counts.2,
        counts.3,
        counts.4,
    )
    .await?;

    // Only clear the exact generation/source snapshot we recomputed. If ingestion refreshed this
    // marker while the rollup was being calculated, the newer row deliberately survives.
    let clear = Query::delete()
        .from_table(Alias::new("telemetry_dirty_days"))
        .and_where(Expr::col(Alias::new("id")).eq(&dirty.id))
        .and_where(Expr::col(Alias::new("generation")).eq(dirty.generation))
        .and_where(Expr::col(Alias::new("source_mask")).eq(dirty.source_mask))
        .to_owned();
    let cleared = transaction.execute(&clear).await?.rows_affected() == 1;
    transaction.commit().await?;
    Ok(cleared)
}

async fn existing_rollup_counts(
    database: &impl ConnectionTrait,
    application_id: &str,
    environment_id: &str,
    day: &str,
) -> Result<(u64, u64, u64, u64, u64), DbErr> {
    let query = Query::select()
        .columns(["events", "users", "metrics", "logs", "errors"].map(Alias::new))
        .from(Alias::new("telemetry_daily_rollups"))
        .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
        .and_where(Expr::col(Alias::new("environment_id")).eq(environment_id))
        .and_where(Expr::col(Alias::new("day")).eq(day))
        .limit(1)
        .to_owned();
    let Some(row) = database.query_one(&query).await? else {
        return Ok((0, 0, 0, 0, 0));
    };
    Ok((
        positive_u64(row.try_get::<i64>("", "events").unwrap_or(0)),
        positive_u64(row.try_get::<i64>("", "users").unwrap_or(0)),
        positive_u64(row.try_get::<i64>("", "metrics").unwrap_or(0)),
        positive_u64(row.try_get::<i64>("", "logs").unwrap_or(0)),
        positive_u64(row.try_get::<i64>("", "errors").unwrap_or(0)),
    ))
}

async fn upsert_rollup(
    database: &impl ConnectionTrait,
    application_id: &str,
    environment_id: &str,
    day: &str,
    events: u64,
    users: u64,
    metrics: u64,
    logs: u64,
    errors: u64,
) -> Result<(), DbErr> {
    let mut query = Query::insert();
    query
        .into_table(Alias::new("telemetry_daily_rollups"))
        .columns(
            [
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
            ]
            .map(Alias::new),
        )
        .values(
            [
                Value::from(rollup_id(application_id, environment_id, day)),
                Value::from(application_id.to_owned()),
                Value::from(environment_id.to_owned()),
                Value::from(day.to_owned()),
                Value::from(saturating_i64(events)),
                Value::from(saturating_i64(users)),
                Value::from(saturating_i64(metrics)),
                Value::from(saturating_i64(logs)),
                Value::from(saturating_i64(errors)),
                Value::from(chrono::Utc::now().timestamp_millis()),
            ]
            .into_iter()
            .map(Expr::value),
        )
        .map_err(|error| DbErr::Custom(error.to_string()))?
        .on_conflict(
            OnConflict::column(Alias::new("id"))
                .update_columns(
                    ["events", "users", "metrics", "logs", "errors", "updated_at"]
                        .map(Alias::new),
                )
                .to_owned(),
        );
    database.execute(&query).await?;
    Ok(())
}

pub async fn seed_historical_dirty_days_once(
    database: &DatabaseConnection,
) -> Result<usize, DbErr> {
    if rollup_backfill_seeded(database).await? {
        return Ok(0);
    }

    let mut scope_days = BTreeSet::<(String, String, String)>::new();
    collect_existing_days(database, "events", Some("day"), &mut scope_days).await?;
    for table in ["metric_points", "logs", "error_occurrences"] {
        collect_existing_days(database, table, None, &mut scope_days).await?;
    }

    let now = chrono::Utc::now().timestamp_millis();
    let mut rows = Vec::with_capacity(scope_days.len().saturating_mul(2));
    for (application_id, environment_id, day) in &scope_days {
        rows.push(dirty_row(
            application_id,
            environment_id,
            day,
            now,
            DIRTY_SOURCE_ALL,
        ));
        rows.push(dirty_row(
            application_id,
            GLOBAL_ENVIRONMENT,
            day,
            now,
            DIRTY_SOURCE_ALL,
        ));
    }
    upsert_dirty_rows(database, rows, DIRTY_SOURCE_ALL).await?;
    set_system_state(database, ROLLUP_BACKFILL_KEY, "complete").await?;
    Ok(scope_days.len())
}

pub async fn rollup_backfill_seeded(database: &DatabaseConnection) -> Result<bool, DbErr> {
    let query = Query::select()
        .column(Alias::new("value"))
        .from(Alias::new("system_state"))
        .and_where(Expr::col(Alias::new("key")).eq(ROLLUP_BACKFILL_KEY))
        .limit(1)
        .to_owned();
    Ok(database
        .query_one(&query)
        .await?
        .and_then(|row| row.try_get::<String>("", "value").ok())
        .is_some_and(|value| value == "complete"))
}

pub async fn invalidate_rollup_backfill(database: &impl ConnectionTrait) -> Result<(), DbErr> {
    let delete = Query::delete()
        .from_table(Alias::new("system_state"))
        .and_where(Expr::col(Alias::new("key")).eq(ROLLUP_BACKFILL_KEY))
        .to_owned();
    database.execute(&delete).await?;
    Ok(())
}

async fn collect_existing_days(
    database: &DatabaseConnection,
    table: &str,
    stored_day_column: Option<&str>,
    output: &mut BTreeSet<(String, String, String)>,
) -> Result<(), DbErr> {
    let day_expression = stored_day_column.map_or_else(
        || timestamp_day_expr(database.get_database_backend()),
        |column| column.to_owned(),
    );
    let mut query = Query::select();
    query
        .columns(["application_id", "environment_id"].map(Alias::new))
        .expr_as(Expr::cust(day_expression), Alias::new("rollup_day"))
        .from(Alias::new(table))
        .distinct();
    let rows = database.query_all(&query).await?;
    for row in rows {
        let application_id: String = row.try_get("", "application_id")?;
        let environment_id: String = row.try_get("", "environment_id")?;
        let day: String = row.try_get("", "rollup_day")?;
        if NaiveDate::parse_from_str(&day, "%Y-%m-%d").is_ok() {
            output.insert((application_id, environment_id, day));
        }
    }
    Ok(())
}

pub async fn application_event_trend_hybrid(
    database: &DatabaseConnection,
    application_id: &str,
    environment_id: Option<&str>,
    since_day: &str,
) -> Result<Option<Vec<DailyRollupPoint>>, DbErr> {
    if !rollup_backfill_seeded(database).await? {
        return Ok(None);
    }
    let rollup_environment = environment_id.unwrap_or(GLOBAL_ENVIRONMENT);

    let rollups = Query::select()
        .columns(
            ["day", "events", "users", "metrics", "logs", "errors"].map(Alias::new),
        )
        .from(Alias::new("telemetry_daily_rollups"))
        .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
        .and_where(Expr::col(Alias::new("environment_id")).eq(rollup_environment))
        .and_where(Expr::col(Alias::new("day")).gte(since_day))
        .order_by(Alias::new("day"), Order::Asc)
        .to_owned();
    let mut points = BTreeMap::<String, DailyRollupPoint>::new();
    for row in database.query_all(&rollups).await? {
        let day: String = row.try_get("", "day")?;
        points.insert(
            day.clone(),
            DailyRollupPoint {
                day,
                events: positive_u64(row.try_get::<i64>("", "events").unwrap_or(0)),
                users: positive_u64(row.try_get::<i64>("", "users").unwrap_or(0)),
                metrics: positive_u64(row.try_get::<i64>("", "metrics").unwrap_or(0)),
                logs: positive_u64(row.try_get::<i64>("", "logs").unwrap_or(0)),
                errors: positive_u64(row.try_get::<i64>("", "errors").unwrap_or(0)),
            },
        );
    }

    let dirty_query = Query::select()
        .column(Alias::new("day"))
        .from(Alias::new("telemetry_dirty_days"))
        .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
        .and_where(Expr::col(Alias::new("environment_id")).eq(rollup_environment))
        .and_where(Expr::col(Alias::new("day")).gte(since_day))
        .and_where(dirty_source_condition(DIRTY_SOURCE_EVENT))
        .to_owned();
    let dirty_days = database
        .query_all(&dirty_query)
        .await?
        .into_iter()
        .filter_map(|row| row.try_get::<String>("", "day").ok())
        .collect::<Vec<_>>();

    if !dirty_days.is_empty() {
        let mut raw = Query::select();
        raw.expr_as(
            Func::count(Expr::col(Alias::new("id"))),
            Alias::new("events"),
        )
        .expr_as(
            Expr::cust("COUNT(DISTINCT anonymous_id)"),
            Alias::new("users"),
        )
        .column(Alias::new("day"))
        .from(Alias::new("events"))
        .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
        .and_where(Expr::col(Alias::new("day")).is_in(dirty_days.iter().map(String::as_str)))
        .group_by_col(Alias::new("day"));
    if let Some(environment_id) = environment_id {
        raw.and_where(Expr::col(Alias::new("environment_id")).eq(environment_id));
    }
        for row in database.query_all(&raw).await? {
            let day: String = row.try_get("", "day")?;
            let existing = points.remove(&day);
            points.insert(
                day.clone(),
                DailyRollupPoint {
                    day,
                    events: positive_u64(row.try_get::<i64>("", "events").unwrap_or(0)),
                    users: positive_u64(row.try_get::<i64>("", "users").unwrap_or(0)),
                    metrics: existing.as_ref().map_or(0, |value| value.metrics),
                    logs: existing.as_ref().map_or(0, |value| value.logs),
                    errors: existing.as_ref().map_or(0, |value| value.errors),
                },
            );
        }
    }

    Ok(Some(points.into_values().collect()))
}

async fn event_counts(
    database: &impl ConnectionTrait,
    application_id: &str,
    environment_id: Option<&str>,
    day: &str,
) -> Result<(u64, u64), DbErr> {
    let mut query = Query::select();
    query
        .expr_as(
            Func::count(Expr::col(Alias::new("id"))),
            Alias::new("events"),
        )
        .expr_as(
            Expr::cust("COUNT(DISTINCT anonymous_id)"),
            Alias::new("users"),
        )
        .from(Alias::new("events"))
        .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
        .and_where(Expr::col(Alias::new("day")).eq(day));
    if let Some(environment_id) = environment_id {
        query.and_where(Expr::col(Alias::new("environment_id")).eq(environment_id));
    }
    let row = database.query_one(&query).await?;
    let events = row
        .as_ref()
        .and_then(|row| row.try_get::<i64>("", "events").ok())
        .unwrap_or(0)
        .max(0) as u64;
    let users = row
        .as_ref()
        .and_then(|row| row.try_get::<i64>("", "users").ok())
        .unwrap_or(0)
        .max(0) as u64;
    Ok((events, users))
}

async fn time_count(
    database: &impl ConnectionTrait,
    table: &str,
    application_id: &str,
    environment_id: Option<&str>,
    start: i64,
    end: i64,
) -> Result<u64, DbErr> {
    let mut query = Query::select();
    query
        .expr_as(
            Func::count(Expr::col(Alias::new("id"))),
            Alias::new("total"),
        )
        .from(Alias::new(table))
        .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
        .and_where(Expr::col(Alias::new("timestamp")).gte(start))
        .and_where(Expr::col(Alias::new("timestamp")).lt(end));
    if let Some(environment_id) = environment_id {
        query.and_where(Expr::col(Alias::new("environment_id")).eq(environment_id));
    }
    let row = database.query_one(&query).await?;
    Ok(row
        .and_then(|row| row.try_get::<i64>("", "total").ok())
        .unwrap_or(0)
        .max(0) as u64)
}

async fn set_system_state(
    database: &impl ConnectionTrait,
    key: &str,
    value: &str,
) -> Result<(), DbErr> {
    let mut query = Query::insert();
    query
        .into_table(Alias::new("system_state"))
        .columns([Alias::new("key"), Alias::new("value")])
        .values(
            [Value::from(key.to_owned()), Value::from(value.to_owned())]
                .into_iter()
                .map(Expr::value),
        )
        .map_err(|error| DbErr::Custom(error.to_string()))?
        .on_conflict(
            OnConflict::column(Alias::new("key"))
                .update_column(Alias::new("value"))
                .to_owned(),
        );
    database.execute(&query).await?;
    Ok(())
}

fn dirty_row(
    application_id: &str,
    environment_id: &str,
    day: &str,
    marked_at: i64,
    source_mask: i64,
) -> Vec<Value> {
    vec![
        Value::from(dirty_id(application_id, environment_id, day)),
        Value::from(application_id.to_owned()),
        Value::from(environment_id.to_owned()),
        Value::from(day.to_owned()),
        Value::from(marked_at),
        Value::from(1_i64),
        Value::from(source_mask),
    ]
}

fn day_bounds(day: &str) -> Result<(i64, i64), DbErr> {
    let date = NaiveDate::parse_from_str(day, "%Y-%m-%d")
        .map_err(|error| DbErr::Custom(error.to_string()))?;
    let start = date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| DbErr::Custom("invalid rollup day".into()))?
        .and_utc()
        .timestamp_millis();
    Ok((start, start.saturating_add(86_400_000)))
}

fn day_for_timestamp(timestamp: i64) -> Option<String> {
    chrono::DateTime::from_timestamp_millis(timestamp)
        .map(|value| value.format("%Y-%m-%d").to_string())
}

fn timestamp_day_expr(backend: DbBackend) -> String {
    match backend {
        DbBackend::Postgres => {
            "to_char(to_timestamp(timestamp / 1000.0), 'YYYY-MM-DD')".into()
        }
        DbBackend::MySql => "DATE_FORMAT(FROM_UNIXTIME(timestamp / 1000), '%Y-%m-%d')".into(),
        DbBackend::Sqlite => "strftime('%Y-%m-%d', timestamp / 1000, 'unixepoch')".into(),
        _ => "''".into(),
    }
}

fn dirty_id(application_id: &str, environment_id: &str, day: &str) -> String {
    scoped_id("dirty", application_id, environment_id, day)
}

fn rollup_id(application_id: &str, environment_id: &str, day: &str) -> String {
    scoped_id("rollup", application_id, environment_id, day)
}

fn scoped_id(kind: &str, application_id: &str, environment_id: &str, day: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"sonde:daily-rollup:v1\0");
    hasher.update(kind.as_bytes());
    hasher.update(b"\0");
    hasher.update(application_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(environment_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(day.as_bytes());
    format!("dr_{}", hex::encode(hasher.finalize()))
}

fn positive_u64(value: i64) -> u64 {
    value.max(0) as u64
}

fn saturating_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{
        DIRTY_SOURCE_EVENT, DIRTY_SOURCE_LOG, DIRTY_SOURCE_LOG_ERROR, DIRTY_SOURCE_VALID,
        day_bounds, day_for_timestamp, dirty_source_condition, rollup_id,
    };

    #[test]
    fn day_round_trip_is_utc_and_stable() {
        let timestamp = 1_767_225_600_000_i64;
        let day = day_for_timestamp(timestamp).unwrap();
        let (start, end) = day_bounds(&day).unwrap();
        assert!(timestamp >= start && timestamp < end);
        assert_eq!(
            rollup_id("app", "prod", &day),
            rollup_id("app", "prod", &day)
        );
    }

    #[test]
    fn dirty_source_masks_are_independent_bits() {
        assert_eq!(DIRTY_SOURCE_EVENT & DIRTY_SOURCE_LOG, 0);
        assert_eq!(DIRTY_SOURCE_LOG_ERROR & DIRTY_SOURCE_LOG, 0);
        assert_eq!(DIRTY_SOURCE_VALID & DIRTY_SOURCE_LOG_ERROR, DIRTY_SOURCE_LOG_ERROR);
        let _ = dirty_source_condition(DIRTY_SOURCE_EVENT | DIRTY_SOURCE_LOG_ERROR);
    }
}
