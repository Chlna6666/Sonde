use std::collections::BTreeMap;

use chrono::NaiveDate;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, DbErr, TransactionTrait,
    sea_query::{Alias, Expr, ExprTrait, Func, OnConflict, Query, Value},
};
use sha2::{Digest, Sha256};

use super::{rollup_repo, telemetry_repo::TelemetryScope};
pub use super::rollup_repo::DIRTY_SOURCE_LOG_ERROR;

const GLOBAL_ENVIRONMENT: &str = "*";
const BACKFILL_KEY: &str = "telemetry_log_error_rollup_backfill_v2";

pub async fn seed_historical_dirty_days_once(
    database: &DatabaseConnection,
) -> Result<usize, DbErr> {
    if backfill_seeded(database).await? {
        return Ok(0);
    }

    let mut query = Query::select();
    query
        .columns(["application_id", "environment_id"].map(Alias::new))
        .expr_as(
            Expr::cust(timestamp_day_expr(database.get_database_backend())),
            Alias::new("rollup_day"),
        )
        .from(Alias::new("logs"))
        .and_where(Expr::col(Alias::new("level")).is_in(["error", "fatal"]))
        .distinct();

    let mut scopes = BTreeMap::<(String, String), Vec<i64>>::new();
    let mut seeded = 0_usize;
    for row in database.query_all(&query).await? {
        let application_id: String = row.try_get("", "application_id")?;
        let environment_id: String = row.try_get("", "environment_id")?;
        let day: String = row.try_get("", "rollup_day")?;
        let Some(timestamp) = day_start_timestamp(&day) else {
            continue;
        };
        scopes
            .entry((application_id, environment_id))
            .or_default()
            .push(timestamp);
        seeded = seeded.saturating_add(1);
    }

    for ((application_id, environment_id), timestamps) in scopes {
        let scope = TelemetryScope {
            application_id,
            environment_id,
        };
        rollup_repo::mark_dirty_timestamps_for_source(
            database,
            &scope,
            DIRTY_SOURCE_LOG_ERROR,
            timestamps,
        )
        .await?;
    }

    set_system_state(database, BACKFILL_KEY, "complete").await?;
    Ok(seeded)
}

pub async fn backfill_seeded(database: &DatabaseConnection) -> Result<bool, DbErr> {
    let query = Query::select()
        .column(Alias::new("value"))
        .from(Alias::new("system_state"))
        .and_where(Expr::col(Alias::new("key")).eq(BACKFILL_KEY))
        .limit(1)
        .to_owned();
    Ok(database
        .query_one(&query)
        .await?
        .and_then(|row| row.try_get::<String>("", "value").ok())
        .is_some_and(|value| value == "complete"))
}

pub async fn invalidate_backfill(database: &DatabaseConnection) -> Result<(), DbErr> {
    let delete = Query::delete()
        .from_table(Alias::new("system_state"))
        .and_where(Expr::col(Alias::new("key")).eq(BACKFILL_KEY))
        .to_owned();
    database.execute(&delete).await?;
    Ok(())
}

pub async fn recompute_claimed_day(
    database: &DatabaseConnection,
    dirty: &rollup_repo::DirtyDay,
) -> Result<bool, DbErr> {
    if !dirty.has_source(DIRTY_SOURCE_LOG_ERROR) {
        return Ok(true);
    }

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
    let mut count = Query::select();
    count
        .expr_as(
            Func::count(Expr::col(Alias::new("id"))),
            Alias::new("total"),
        )
        .from(Alias::new("logs"))
        .and_where(Expr::col(Alias::new("application_id")).eq(&dirty.application_id))
        .and_where(Expr::col(Alias::new("timestamp")).gte(start))
        .and_where(Expr::col(Alias::new("timestamp")).lt(end))
        .and_where(Expr::col(Alias::new("level")).is_in(["error", "fatal"]));
    if dirty.environment_id != GLOBAL_ENVIRONMENT {
        count.and_where(Expr::col(Alias::new("environment_id")).eq(&dirty.environment_id));
    }
    let error_logs = transaction
        .query_one(&count)
        .await?
        .and_then(|row| row.try_get::<i64>("", "total").ok())
        .unwrap_or(0)
        .max(0);

    let mut upsert = Query::insert();
    upsert
        .into_table(Alias::new("telemetry_daily_log_errors"))
        .columns(
            [
                "id",
                "application_id",
                "environment_id",
                "day",
                "error_logs",
                "updated_at",
            ]
            .map(Alias::new),
        )
        .values(
            [
                Value::from(rollup_id(
                    &dirty.application_id,
                    &dirty.environment_id,
                    &dirty.day,
                )),
                Value::from(dirty.application_id.clone()),
                Value::from(dirty.environment_id.clone()),
                Value::from(dirty.day.clone()),
                Value::from(error_logs),
                Value::from(chrono::Utc::now().timestamp_millis()),
            ]
            .into_iter()
            .map(Expr::value),
        )
        .map_err(|error| DbErr::Custom(error.to_string()))?
        .on_conflict(
            OnConflict::column(Alias::new("id"))
                .update_columns(["error_logs", "updated_at"].map(Alias::new))
                .to_owned(),
        );
    transaction.execute(&upsert).await?;
    transaction.commit().await?;
    Ok(true)
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

fn rollup_id(application_id: &str, environment_id: &str, day: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"sonde:log-error-rollup:v1\0");
    for part in [application_id, environment_id, day] {
        hasher.update(part.as_bytes());
        hasher.update(b"\0");
    }
    format!("le_{}", hex::encode(hasher.finalize()))
}

fn day_bounds(day: &str) -> Result<(i64, i64), DbErr> {
    let date = NaiveDate::parse_from_str(day, "%Y-%m-%d")
        .map_err(|error| DbErr::Custom(error.to_string()))?;
    let start = date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| DbErr::Custom("invalid log error rollup day".into()))?
        .and_utc()
        .timestamp_millis();
    Ok((start, start.saturating_add(86_400_000)))
}

fn day_start_timestamp(day: &str) -> Option<i64> {
    Some(
        NaiveDate::parse_from_str(day, "%Y-%m-%d")
            .ok()?
            .and_hms_opt(0, 0, 0)?
            .and_utc()
            .timestamp_millis(),
    )
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
