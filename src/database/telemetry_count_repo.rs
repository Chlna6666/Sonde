use std::collections::{BTreeMap, HashSet};

use chrono::{DateTime, NaiveDate, Utc};
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr,
    sea_query::{Alias, Expr, ExprTrait, Func, Order, Query, SelectStatement, SimpleExpr},
};

use super::{log_error_rollup_repo, rollup_repo};

const GLOBAL_ENVIRONMENT: &str = "*";
const MAX_DIRTY_DAY_BINDS: usize = 400;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RollupCountKind {
    Events,
    Metrics,
    Logs,
    ErrorLogs,
}

impl RollupCountKind {
    fn raw_table(self) -> &'static str {
        match self {
            Self::Events => "events",
            Self::Metrics => "metric_points",
            Self::Logs | Self::ErrorLogs => "logs",
        }
    }

    fn rollup_table(self) -> &'static str {
        match self {
            Self::Events | Self::Metrics | Self::Logs => "telemetry_daily_rollups",
            Self::ErrorLogs => "telemetry_daily_log_errors",
        }
    }

    fn rollup_column(self) -> &'static str {
        match self {
            Self::Events => "events",
            Self::Metrics => "metrics",
            Self::Logs => "logs",
            Self::ErrorLogs => "error_logs",
        }
    }

    fn source_mask(self) -> i64 {
        match self {
            Self::Events => rollup_repo::DIRTY_SOURCE_EVENT,
            Self::Metrics => rollup_repo::DIRTY_SOURCE_METRIC,
            Self::Logs | Self::ErrorLogs => rollup_repo::DIRTY_SOURCE_LOG,
        }
    }

    fn uses_stored_day(self) -> bool {
        matches!(self, Self::Events)
    }

    fn apply_raw_filter(self, query: &mut SelectStatement) {
        if matches!(self, Self::ErrorLogs) {
            query.and_where(Expr::col(Alias::new("level")).is_in(["error", "fatal"]));
        }
    }
}

/// Exact scalar count over `[since, until)` backed by daily rollups for complete clean days.
///
/// Each kind declares its raw table, rollup source and dirty bit. Dirty replacement is therefore
/// source-aware: a metric-only marker cannot force event/log scans, while `ErrorLogs` preserves the
/// Dashboard definition of errors as log rows whose level is `error` or `fatal`.
pub async fn count_hybrid(
    database: &DatabaseConnection,
    kind: RollupCountKind,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since: Option<i64>,
    until: Option<i64>,
) -> Result<u64, DbErr> {
    if application_id.is_none() && environment_id.is_some() {
        return Err(DbErr::Custom(
            "environment-scoped telemetry count requires application id".into(),
        ));
    }
    if matches!((since, until), (Some(start), Some(end)) if end <= start) {
        return Ok(0);
    }
    if !backfill_ready(database, kind).await? {
        return raw_count(database, kind, application_id, environment_id, since, until).await;
    }

    let start_day = since.and_then(day_for_timestamp);
    let end_day = until
        .and_then(|value| value.checked_sub(1))
        .and_then(day_for_timestamp);
    let rollup_environment = environment_id.unwrap_or(GLOBAL_ENVIRONMENT);

    // Read per-application rollup rows and combine them in Rust for global counts. PostgreSQL
    // promotes SUM(BIGINT) to NUMERIC, which is deliberately avoided so all three supported
    // databases expose the same i64 row type and overflow behavior is explicit via saturating_add.
    let mut query = Query::select();
    query
        .columns(["day", kind.rollup_column()].map(Alias::new))
        .from(Alias::new(kind.rollup_table()))
        .and_where(Expr::col(Alias::new("environment_id")).eq(rollup_environment));
    if let Some(application_id) = application_id {
        query.and_where(Expr::col(Alias::new("application_id")).eq(application_id));
    }
    if let Some(day) = start_day.as_deref() {
        query.and_where(Expr::col(Alias::new("day")).gte(day));
    }
    if let Some(day) = end_day.as_deref() {
        query.and_where(Expr::col(Alias::new("day")).lte(day));
    }
    query.order_by(Alias::new("day"), Order::Asc);

    let mut counts = BTreeMap::<String, u64>::new();
    for row in database.query_all(&query).await? {
        let day: String = row.try_get("", "day")?;
        let value = positive_u64(row.try_get::<i64>("", kind.rollup_column()).unwrap_or(0));
        let entry = counts.entry(day).or_insert(0);
        *entry = entry.saturating_add(value);
    }

    if !replace_dirty_days(
        database,
        kind,
        &mut counts,
        application_id,
        environment_id,
        start_day.as_deref(),
        end_day.as_deref(),
    )
    .await?
    {
        return raw_count(database, kind, application_id, environment_id, since, until).await;
    }

    replace_partial_boundaries(
        database,
        kind,
        &mut counts,
        application_id,
        environment_id,
        since,
        until,
    )
    .await?;

    Ok(counts
        .into_values()
        .fold(0_u64, |total, value| total.saturating_add(value)))
}

async fn backfill_ready(
    database: &DatabaseConnection,
    kind: RollupCountKind,
) -> Result<bool, DbErr> {
    match kind {
        RollupCountKind::ErrorLogs => log_error_rollup_repo::backfill_seeded(database).await,
        RollupCountKind::Events | RollupCountKind::Metrics | RollupCountKind::Logs => {
            rollup_repo::rollup_backfill_seeded(database).await
        }
    }
}

async fn replace_dirty_days(
    database: &DatabaseConnection,
    kind: RollupCountKind,
    counts: &mut BTreeMap<String, u64>,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    start_day: Option<&str>,
    end_day: Option<&str>,
) -> Result<bool, DbErr> {
    let dirty_environment = environment_id.unwrap_or(GLOBAL_ENVIRONMENT);
    let mut dirty = Query::select();
    dirty
        .column(Alias::new("day"))
        .from(Alias::new("telemetry_dirty_days"))
        .and_where(Expr::col(Alias::new("environment_id")).eq(dirty_environment))
        .and_where(rollup_repo::dirty_source_condition(kind.source_mask()))
        .distinct()
        .limit((MAX_DIRTY_DAY_BINDS + 1) as u64);
    if let Some(application_id) = application_id {
        dirty.and_where(Expr::col(Alias::new("application_id")).eq(application_id));
    }
    if let Some(day) = start_day {
        dirty.and_where(Expr::col(Alias::new("day")).gte(day));
    }
    if let Some(day) = end_day {
        dirty.and_where(Expr::col(Alias::new("day")).lte(day));
    }
    let dirty_days = database
        .query_all(&dirty)
        .await?
        .into_iter()
        .filter_map(|row| row.try_get::<String>("", "day").ok())
        .collect::<Vec<_>>();
    if dirty_days.is_empty() {
        return Ok(true);
    }
    if dirty_days.len() > MAX_DIRTY_DAY_BINDS {
        return Ok(false);
    }

    let day_expression = raw_day_expression(database, kind);
    let mut raw = Query::select();
    raw.expr_as(
        Func::count(Expr::col(Alias::new("id"))),
        Alias::new("total"),
    )
    .from(Alias::new(kind.raw_table()))
    .and_where(day_expression.clone().is_in(dirty_days.iter().map(String::as_str)))
    .expr_as(day_expression, Alias::new("day"))
    .group_by_col(Alias::new("day"));
    if let Some(application_id) = application_id {
        raw.and_where(Expr::col(Alias::new("application_id")).eq(application_id));
    }
    if let Some(environment_id) = environment_id {
        raw.and_where(Expr::col(Alias::new("environment_id")).eq(environment_id));
    }
    kind.apply_raw_filter(&mut raw);

    let mut seen = HashSet::new();
    for row in database.query_all(&raw).await? {
        let day: String = row.try_get("", "day")?;
        seen.insert(day.clone());
        counts.insert(
            day,
            positive_u64(row.try_get::<i64>("", "total").unwrap_or(0)),
        );
    }
    for day in dirty_days {
        if !seen.contains(&day) {
            counts.remove(&day);
        }
    }
    Ok(true)
}

async fn replace_partial_boundaries(
    database: &DatabaseConnection,
    kind: RollupCountKind,
    counts: &mut BTreeMap<String, u64>,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since: Option<i64>,
    until: Option<i64>,
) -> Result<(), DbErr> {
    let start_day = since.and_then(day_for_timestamp);
    let end_day = until
        .and_then(|value| value.checked_sub(1))
        .and_then(day_for_timestamp);

    if let (Some(since), Some(start_day), Some(end_day)) =
        (since, start_day.as_deref(), end_day.as_deref())
        && start_day == end_day
    {
        let end = until.unwrap_or_else(|| next_day_timestamp(start_day).unwrap_or(i64::MAX));
        let count = raw_count(
            database,
            kind,
            application_id,
            environment_id,
            Some(since),
            Some(end),
        )
        .await?;
        replace_day(counts, start_day, count);
        return Ok(());
    }

    if let Some(since) = since
        && let Some(day) = start_day.as_deref()
        && day_start_timestamp(day).is_some_and(|start| since > start)
    {
        let day_end = next_day_timestamp(day).unwrap_or(i64::MAX);
        let count = raw_count(
            database,
            kind,
            application_id,
            environment_id,
            Some(since),
            Some(until.map_or(day_end, |value| value.min(day_end))),
        )
        .await?;
        replace_day(counts, day, count);
    }

    if let Some(until) = until
        && let Some(day) = end_day.as_deref()
        && day_start_timestamp(day).is_some_and(|start| until > start)
        && next_day_timestamp(day).is_some_and(|end| until < end)
    {
        let day_start = day_start_timestamp(day).unwrap_or(i64::MIN);
        let count = raw_count(
            database,
            kind,
            application_id,
            environment_id,
            Some(since.map_or(day_start, |value| value.max(day_start))),
            Some(until),
        )
        .await?;
        replace_day(counts, day, count);
    }
    Ok(())
}

async fn raw_count(
    database: &DatabaseConnection,
    kind: RollupCountKind,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since: Option<i64>,
    until: Option<i64>,
) -> Result<u64, DbErr> {
    let mut query = Query::select();
    query
        .expr_as(
            Func::count(Expr::col(Alias::new("id"))),
            Alias::new("total"),
        )
        .from(Alias::new(kind.raw_table()));
    if let Some(application_id) = application_id {
        query.and_where(Expr::col(Alias::new("application_id")).eq(application_id));
    }
    if let Some(environment_id) = environment_id {
        query.and_where(Expr::col(Alias::new("environment_id")).eq(environment_id));
    }
    if let Some(since) = since {
        query.and_where(Expr::col(Alias::new("timestamp")).gte(since));
    }
    if let Some(until) = until {
        query.and_where(Expr::col(Alias::new("timestamp")).lt(until));
    }
    kind.apply_raw_filter(&mut query);
    let row = database.query_one(&query).await?;
    Ok(row
        .and_then(|row| row.try_get::<i64>("", "total").ok())
        .unwrap_or(0)
        .max(0) as u64)
}

fn raw_day_expression(database: &DatabaseConnection, kind: RollupCountKind) -> SimpleExpr {
    if kind.uses_stored_day() {
        return Expr::col(Alias::new("day"));
    }
    match database.get_database_backend() {
        sea_orm::DbBackend::Postgres => {
            Expr::cust("to_char(to_timestamp(timestamp / 1000.0), 'YYYY-MM-DD')")
        }
        sea_orm::DbBackend::MySql => {
            Expr::cust("DATE_FORMAT(FROM_UNIXTIME(timestamp / 1000), '%Y-%m-%d')")
        }
        sea_orm::DbBackend::Sqlite => {
            Expr::cust("strftime('%Y-%m-%d', timestamp / 1000, 'unixepoch')")
        }
        _ => Expr::cust("''"),
    }
}

fn replace_day(counts: &mut BTreeMap<String, u64>, day: &str, count: u64) {
    if count == 0 {
        counts.remove(day);
    } else {
        counts.insert(day.to_owned(), count);
    }
}

fn day_for_timestamp(timestamp: i64) -> Option<String> {
    DateTime::<Utc>::from_timestamp_millis(timestamp)
        .map(|value| value.format("%Y-%m-%d").to_string())
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

fn next_day_timestamp(day: &str) -> Option<i64> {
    let date = NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()?;
    let next = date.succ_opt()?;
    Some(next.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis())
}

fn positive_u64(value: i64) -> u64 {
    value.max(0) as u64
}

#[cfg(test)]
mod tests {
    use super::RollupCountKind;

    #[test]
    fn kinds_have_source_and_storage_contracts() {
        assert_ne!(
            RollupCountKind::Metrics.source_mask(),
            RollupCountKind::Logs.source_mask()
        );
        assert_eq!(
            RollupCountKind::Logs.source_mask(),
            RollupCountKind::ErrorLogs.source_mask()
        );
        assert!(RollupCountKind::Events.uses_stored_day());
        assert_eq!(
            RollupCountKind::ErrorLogs.rollup_table(),
            "telemetry_daily_log_errors"
        );
    }
}
