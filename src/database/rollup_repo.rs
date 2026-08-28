use std::collections::BTreeSet;

use chrono::NaiveDate;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr, TransactionTrait,
    sea_query::{Alias, Expr, ExprTrait, Func, Order, Query, Value},
};
use sha2::{Digest, Sha256};

use super::{
    query::{insert, insert_batch_ignore_conflicts},
    telemetry_repo::TelemetryScope,
};

const GLOBAL_ENVIRONMENT: &str = "*";

#[derive(Clone, Debug)]
pub struct DirtyDay {
    pub id: String,
    pub application_id: String,
    pub environment_id: String,
    pub day: String,
}

pub async fn mark_dirty_timestamps<I>(
    database: &impl ConnectionTrait,
    scope: &TelemetryScope,
    timestamps: I,
) -> Result<(), DbErr>
where
    I: IntoIterator<Item = i64>,
{
    let days: BTreeSet<String> = timestamps.into_iter().filter_map(day_for_timestamp).collect();
    if days.is_empty() {
        return Ok(());
    }
    let now = chrono::Utc::now().timestamp_millis();
    let mut rows = Vec::with_capacity(days.len() * 2);
    for day in days {
        for environment_id in [scope.environment_id.as_str(), GLOBAL_ENVIRONMENT] {
            rows.push(vec![
                Value::from(dirty_id(&scope.application_id, environment_id, &day)),
                Value::from(scope.application_id.clone()),
                Value::from(environment_id.to_owned()),
                Value::from(day.clone()),
                Value::from(now),
            ]);
        }
    }
    insert_batch_ignore_conflicts(
        database,
        "telemetry_dirty_days",
        &["id", "application_id", "environment_id", "day", "marked_at"],
        rows,
        "id",
        "id",
    )
    .await?;
    Ok(())
}

pub async fn list_dirty_days(
    database: &DatabaseConnection,
    limit: u64,
) -> Result<Vec<DirtyDay>, DbErr> {
    let query = Query::select()
        .columns(["id", "application_id", "environment_id", "day"].map(Alias::new))
        .from(Alias::new("telemetry_dirty_days"))
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
            })
        })
        .collect()
}

pub async fn recompute_claimed_day(
    database: &DatabaseConnection,
    dirty: DirtyDay,
) -> Result<bool, DbErr> {
    let transaction = database.begin().await?;
    let claim = Query::delete()
        .from_table(Alias::new("telemetry_dirty_days"))
        .and_where(Expr::col(Alias::new("id")).eq(&dirty.id))
        .to_owned();
    if transaction.execute(&claim).await?.rows_affected() != 1 {
        transaction.rollback().await?;
        return Ok(false);
    }

    let (start, end) = day_bounds(&dirty.day)?;
    let environment = (dirty.environment_id != GLOBAL_ENVIRONMENT)
        .then_some(dirty.environment_id.as_str());
    let (events, users) = event_counts(
        &transaction,
        &dirty.application_id,
        environment,
        &dirty.day,
    )
    .await?;
    let metrics = time_count(
        &transaction,
        "metric_points",
        &dirty.application_id,
        environment,
        start,
        end,
    )
    .await?;
    let logs = time_count(
        &transaction,
        "logs",
        &dirty.application_id,
        environment,
        start,
        end,
    )
    .await?;
    let errors = time_count(
        &transaction,
        "error_occurrences",
        &dirty.application_id,
        environment,
        start,
        end,
    )
    .await?;

    let rollup_id = rollup_id(
        &dirty.application_id,
        &dirty.environment_id,
        &dirty.day,
    );
    let delete_existing = Query::delete()
        .from_table(Alias::new("telemetry_daily_rollups"))
        .and_where(Expr::col(Alias::new("id")).eq(&rollup_id))
        .to_owned();
    transaction.execute(&delete_existing).await?;
    insert(
        &transaction,
        "telemetry_daily_rollups",
        &[
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
        vec![
            Value::from(rollup_id),
            Value::from(dirty.application_id),
            Value::from(dirty.environment_id),
            Value::from(dirty.day),
            Value::from(i64::try_from(events).unwrap_or(i64::MAX)),
            Value::from(i64::try_from(users).unwrap_or(i64::MAX)),
            Value::from(i64::try_from(metrics).unwrap_or(i64::MAX)),
            Value::from(i64::try_from(logs).unwrap_or(i64::MAX)),
            Value::from(i64::try_from(errors).unwrap_or(i64::MAX)),
            Value::from(chrono::Utc::now().timestamp_millis()),
        ],
    )
    .await?;
    transaction.commit().await?;
    Ok(true)
}

async fn event_counts(
    database: &impl ConnectionTrait,
    application_id: &str,
    environment_id: Option<&str>,
    day: &str,
) -> Result<(u64, u64), DbErr> {
    let mut query = Query::select();
    query
        .expr_as(Func::count(Expr::col(Alias::new("id"))), Alias::new("events"))
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
        .expr_as(Func::count(Expr::col(Alias::new("id"))), Alias::new("total"))
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

#[cfg(test)]
mod tests {
    use super::{day_bounds, day_for_timestamp, rollup_id};

    #[test]
    fn day_round_trip_is_utc_and_stable() {
        let timestamp = 1_767_225_600_000_i64;
        let day = day_for_timestamp(timestamp).unwrap();
        let (start, end) = day_bounds(&day).unwrap();
        assert!(timestamp >= start && timestamp < end);
        assert_eq!(rollup_id("app", "prod", &day), rollup_id("app", "prod", &day));
    }
}
