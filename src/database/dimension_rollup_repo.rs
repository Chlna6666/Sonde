use std::collections::{BTreeMap, BTreeSet};

use chrono::NaiveDate;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr, TransactionTrait,
    sea_query::{Alias, Expr, ExprTrait, Func, OnConflict, Order, Query, Value},
};
use sha2::{Digest, Sha256};

use super::{query::insert_batch, rollup_repo::DirtyDay};

pub const DIMENSION_APP_VERSION: &str = "app_version";
pub const DIMENSION_LAUNCHER_VERSION: &str = "launcher_version";
pub const DIMENSION_OS: &str = "os";

const GLOBAL_ENVIRONMENT: &str = "*";
const DIMENSION_BACKFILL_KEY: &str = "telemetry_dimension_rollup_backfill_v1";
const MAX_DIRTY_SCOPE_DAYS: usize = 32;
const DIRTY_INSERT_CHUNK: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DimensionDayCount {
    pub day: String,
    pub value: String,
    pub count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DimensionCount {
    pub value: String,
    pub count: u64,
}

pub async fn seed_historical_dimension_dirty_days_once(
    database: &DatabaseConnection,
) -> Result<usize, DbErr> {
    if dimension_backfill_seeded(database).await? {
        return Ok(0);
    }

    let query = Query::select()
        .columns(["application_id", "environment_id", "day"].map(Alias::new))
        .from(Alias::new("events"))
        .distinct()
        .to_owned();
    let rows = database.query_all(&query).await?;
    let mut scope_days = BTreeSet::new();
    for row in rows {
        let application_id: String = row.try_get("", "application_id")?;
        let environment_id: String = row.try_get("", "environment_id")?;
        let day: String = row.try_get("", "day")?;
        if NaiveDate::parse_from_str(&day, "%Y-%m-%d").is_ok() {
            scope_days.insert((application_id, environment_id, day));
        }
    }

    let now = chrono::Utc::now().timestamp_millis();
    let mut dirty_rows = Vec::with_capacity(scope_days.len().saturating_mul(2));
    for (application_id, environment_id, day) in &scope_days {
        dirty_rows.push(dirty_row(application_id, environment_id, day, now));
        dirty_rows.push(dirty_row(application_id, GLOBAL_ENVIRONMENT, day, now));
    }
    upsert_dirty_rows(database, dirty_rows).await?;
    set_system_state(database, DIMENSION_BACKFILL_KEY, "complete").await?;
    Ok(scope_days.len())
}

pub async fn dimension_backfill_seeded(database: &DatabaseConnection) -> Result<bool, DbErr> {
    let query = Query::select()
        .column(Alias::new("value"))
        .from(Alias::new("system_state"))
        .and_where(Expr::col(Alias::new("key")).eq(DIMENSION_BACKFILL_KEY))
        .limit(1)
        .to_owned();
    Ok(database
        .query_one(&query)
        .await?
        .and_then(|row| row.try_get::<String>("", "value").ok())
        .is_some_and(|value| value == "complete"))
}

pub async fn invalidate_dimension_backfill(database: &DatabaseConnection) -> Result<(), DbErr> {
    let delete = Query::delete()
        .from_table(Alias::new("system_state"))
        .and_where(Expr::col(Alias::new("key")).eq(DIMENSION_BACKFILL_KEY))
        .to_owned();
    database.execute(&delete).await?;
    Ok(())
}

pub async fn recompute_claimed_day_dimensions(
    database: &DatabaseConnection,
    dirty: &DirtyDay,
) -> Result<bool, DbErr> {
    let transaction = database.begin().await?;
    let marker = Query::select()
        .column(Alias::new("generation"))
        .from(Alias::new("telemetry_dirty_days"))
        .and_where(Expr::col(Alias::new("id")).eq(&dirty.id))
        .limit(1)
        .to_owned();
    let generation = transaction
        .query_one(&marker)
        .await?
        .and_then(|row| row.try_get::<i64>("", "generation").ok());
    if generation != Some(dirty.generation) {
        transaction.rollback().await?;
        return Ok(false);
    }

    let environment_filter = (dirty.environment_id != GLOBAL_ENVIRONMENT)
        .then_some(dirty.environment_id.as_str());
    let delete = Query::delete()
        .from_table(Alias::new("telemetry_daily_dimensions"))
        .and_where(Expr::col(Alias::new("application_id")).eq(&dirty.application_id))
        .and_where(Expr::col(Alias::new("environment_id")).eq(&dirty.environment_id))
        .and_where(Expr::col(Alias::new("day")).eq(&dirty.day))
        .to_owned();
    transaction.execute(&delete).await?;

    let now = chrono::Utc::now().timestamp_millis();
    let mut rows = Vec::new();
    for dimension in [
        DIMENSION_APP_VERSION,
        DIMENSION_LAUNCHER_VERSION,
        DIMENSION_OS,
    ] {
        let mut query = Query::select();
        query
            .expr_as(
                Expr::cust(format!("COALESCE({dimension}, 'unknown')")),
                Alias::new("dimension_value"),
            )
            .expr_as(
                Func::count(Expr::col(Alias::new("id"))),
                Alias::new("item_count"),
            )
            .from(Alias::new("events"))
            .and_where(Expr::col(Alias::new("application_id")).eq(&dirty.application_id))
            .and_where(Expr::col(Alias::new("day")).eq(&dirty.day))
            .group_by_col(Alias::new("dimension_value"));
        if let Some(environment_id) = environment_filter {
            query.and_where(Expr::col(Alias::new("environment_id")).eq(environment_id));
        }
        for row in transaction.query_all(&query).await? {
            let value: String = row.try_get("", "dimension_value")?;
            let count = positive_u64(row.try_get::<i64>("", "item_count").unwrap_or(0));
            rows.push(vec![
                Value::from(dimension_id(
                    &dirty.application_id,
                    &dirty.environment_id,
                    &dirty.day,
                    dimension,
                    &value,
                )),
                Value::from(dirty.application_id.clone()),
                Value::from(dirty.environment_id.clone()),
                Value::from(dirty.day.clone()),
                Value::from(dimension.to_owned()),
                Value::from(value),
                Value::from(saturating_i64(count)),
                Value::from(now),
            ]);
        }
    }

    insert_batch(
        &transaction,
        "telemetry_daily_dimensions",
        &[
            "id",
            "application_id",
            "environment_id",
            "day",
            "dimension",
            "dimension_value",
            "count",
            "updated_at",
        ],
        rows,
    )
    .await?;
    transaction.commit().await?;
    Ok(true)
}

pub async fn event_dimension_timeline_hybrid(
    database: &DatabaseConnection,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since_ts: Option<i64>,
    dimension: &str,
) -> Result<Option<Vec<DimensionDayCount>>, DbErr> {
    raw_column(dimension)?;
    if !dimension_backfill_seeded(database).await? {
        return Ok(None);
    }

    let rollup_environment = environment_id.unwrap_or(GLOBAL_ENVIRONMENT);
    let since_day = since_ts.and_then(day_for_timestamp);
    let mut query = Query::select();
    query
        .columns(
            ["application_id", "day", "dimension_value", "count"].map(Alias::new),
        )
        .from(Alias::new("telemetry_daily_dimensions"))
        .and_where(Expr::col(Alias::new("environment_id")).eq(rollup_environment))
        .and_where(Expr::col(Alias::new("dimension")).eq(dimension));
    if let Some(application_id) = application_id {
        query.and_where(Expr::col(Alias::new("application_id")).eq(application_id));
    }
    if let Some(since_day) = since_day.as_deref() {
        query.and_where(Expr::col(Alias::new("day")).gte(since_day));
    }
    query.order_by(Alias::new("day"), Order::Asc);

    let mut values = BTreeMap::<(String, String, String), u64>::new();
    for row in database.query_all(&query).await? {
        let app: String = row.try_get("", "application_id")?;
        let day: String = row.try_get("", "day")?;
        let value: String = row.try_get("", "dimension_value")?;
        let count = positive_u64(row.try_get::<i64>("", "count").unwrap_or(0));
        values.insert((app, day, value), count);
    }

    let mut dirty_query = Query::select();
    dirty_query
        .columns(["application_id", "day"].map(Alias::new))
        .from(Alias::new("telemetry_dirty_days"))
        .and_where(Expr::col(Alias::new("environment_id")).eq(rollup_environment));
    if let Some(application_id) = application_id {
        dirty_query.and_where(Expr::col(Alias::new("application_id")).eq(application_id));
    }
    if let Some(since_day) = since_day.as_deref() {
        dirty_query.and_where(Expr::col(Alias::new("day")).gte(since_day));
    }
    let mut dirty = database
        .query_all(&dirty_query)
        .await?
        .into_iter()
        .filter_map(|row| {
            Some((
                row.try_get::<String>("", "application_id").ok()?,
                row.try_get::<String>("", "day").ok()?,
            ))
        })
        .collect::<BTreeSet<_>>();

    if let (Some(since), Some(boundary_day)) = (since_ts, since_day.as_deref()) {
        let (day_start, day_end) = day_bounds(boundary_day)?;
        if since > day_start && since < day_end {
            values.retain(|(_, day, _), _| day != boundary_day);
            for (app, value, count) in raw_dimension_counts(
                database,
                application_id,
                environment_id,
                dimension,
                Some((since, day_end)),
                None,
            )
            .await?
            {
                values.insert((app, boundary_day.to_owned(), value), count);
            }
            dirty.retain(|(_, day)| day != boundary_day);
        }
    }

    if dirty.len() > MAX_DIRTY_SCOPE_DAYS {
        return Ok(None);
    }
    for (app, day) in dirty {
        values.retain(|(stored_app, stored_day, _), _| stored_app != &app || stored_day != &day);
        for (_, value, count) in raw_dimension_counts(
            database,
            Some(&app),
            environment_id,
            dimension,
            None,
            Some(&day),
        )
        .await?
        {
            values.insert((app.clone(), day.clone(), value), count);
        }
    }

    let mut merged = BTreeMap::<(String, String), u64>::new();
    for ((_, day, value), count) in values {
        let entry = merged.entry((day, value)).or_insert(0);
        *entry = entry.saturating_add(count);
    }
    Ok(Some(
        merged
            .into_iter()
            .map(|((day, value), count)| DimensionDayCount { day, value, count })
            .collect(),
    ))
}

pub fn aggregate_dimension(points: &[DimensionDayCount]) -> Vec<DimensionCount> {
    aggregate_transformed(points, |value| value.to_owned())
}

pub fn aggregate_os_families(points: &[DimensionDayCount]) -> Vec<DimensionCount> {
    aggregate_transformed(points, os_family_name)
}

pub fn aggregate_os_builds(points: &[DimensionDayCount]) -> Vec<DimensionCount> {
    aggregate_transformed(points, os_build_name)
}

fn aggregate_transformed<F>(points: &[DimensionDayCount], transform: F) -> Vec<DimensionCount>
where
    F: Fn(&str) -> String,
{
    let mut counts = BTreeMap::<String, u64>::new();
    for point in points {
        let entry = counts.entry(transform(&point.value)).or_insert(0);
        *entry = entry.saturating_add(point.count);
    }
    let mut result = counts
        .into_iter()
        .map(|(value, count)| DimensionCount { value, count })
        .collect::<Vec<_>>();
    result.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.value.cmp(&right.value))
    });
    result
}

fn os_family_name(os: &str) -> String {
    if os.starts_with("Windows") {
        "Windows".into()
    } else if os.starts_with("Linux")
        || os.contains("Linux")
        || os.contains("Fedora")
        || os.contains("Ubuntu")
        || os.contains("Debian")
        || os.contains("Arch")
    {
        "Linux".into()
    } else if os.starts_with("Mac") || os.starts_with("Darwin") || os.starts_with("macOS") {
        "macOS".into()
    } else if os.starts_with("Android") {
        "Android".into()
    } else if os.starts_with("iOS") {
        "iOS".into()
    } else if os.eq_ignore_ascii_case("unknown") {
        "Unknown".into()
    } else {
        os.to_owned()
    }
}

fn os_build_name(os: &str) -> String {
    if let Some(rest) = os.strip_prefix("Windows ") {
        if let Some((version, build)) = rest.split_once(" Build ") {
            return format!("Win {version} ({build})");
        }
        return os.to_owned();
    }
    if let Some(inner) = os.strip_prefix("Linux (").and_then(|value| value.strip_suffix(')')) {
        return inner.strip_suffix(" Linux").unwrap_or(inner).to_owned();
    }
    if let Some(version) = os.strip_prefix("Mac OS X ") {
        return format!("macOS {version}");
    }
    if let Some(version) = os.strip_prefix("Darwin ") {
        return format!("macOS {version}");
    }
    if os.eq_ignore_ascii_case("unknown") {
        return "Unknown".into();
    }
    os.to_owned()
}

async fn raw_dimension_counts(
    database: &impl ConnectionTrait,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    dimension: &str,
    timestamp_range: Option<(i64, i64)>,
    day: Option<&str>,
) -> Result<Vec<(String, String, u64)>, DbErr> {
    let column = raw_column(dimension)?;
    let mut query = Query::select();
    query
        .column(Alias::new("application_id"))
        .expr_as(
            Expr::cust(format!("COALESCE({column}, 'unknown')")),
            Alias::new("dimension_value"),
        )
        .expr_as(
            Func::count(Expr::col(Alias::new("id"))),
            Alias::new("item_count"),
        )
        .from(Alias::new("events"));
    if let Some(application_id) = application_id {
        query.and_where(Expr::col(Alias::new("application_id")).eq(application_id));
    }
    if let Some(environment_id) = environment_id {
        query.and_where(Expr::col(Alias::new("environment_id")).eq(environment_id));
    }
    if let Some((start, end)) = timestamp_range {
        query
            .and_where(Expr::col(Alias::new("timestamp")).gte(start))
            .and_where(Expr::col(Alias::new("timestamp")).lt(end));
    }
    if let Some(day) = day {
        query.and_where(Expr::col(Alias::new("day")).eq(day));
    }
    query
        .group_by_col(Alias::new("application_id"))
        .group_by_col(Alias::new("dimension_value"));

    database
        .query_all(&query)
        .await?
        .into_iter()
        .map(|row| {
            Ok((
                row.try_get("", "application_id")?,
                row.try_get("", "dimension_value")?,
                positive_u64(row.try_get::<i64>("", "item_count").unwrap_or(0)),
            ))
        })
        .collect()
}

fn raw_column(dimension: &str) -> Result<&'static str, DbErr> {
    match dimension {
        DIMENSION_APP_VERSION => Ok("app_version"),
        DIMENSION_LAUNCHER_VERSION => Ok("launcher_version"),
        DIMENSION_OS => Ok("os"),
        _ => Err(DbErr::Custom(format!(
            "unsupported event dimension {dimension}"
        ))),
    }
}

async fn upsert_dirty_rows(
    database: &impl ConnectionTrait,
    rows: Vec<Vec<Value>>,
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
                .values([(
                    Alias::new("generation"),
                    Expr::col(Alias::new("generation")).add(1_i64),
                )])
                .to_owned(),
        );
        database.execute(&query).await?;
    }
    Ok(())
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

fn dirty_row(application_id: &str, environment_id: &str, day: &str, marked_at: i64) -> Vec<Value> {
    vec![
        Value::from(dirty_id(application_id, environment_id, day)),
        Value::from(application_id.to_owned()),
        Value::from(environment_id.to_owned()),
        Value::from(day.to_owned()),
        Value::from(marked_at),
        Value::from(1_i64),
    ]
}

fn dirty_id(application_id: &str, environment_id: &str, day: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"sonde:daily-rollup:v1\0dirty\0");
    hasher.update(application_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(environment_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(day.as_bytes());
    format!("dr_{}", hex::encode(hasher.finalize()))
}

fn dimension_id(
    application_id: &str,
    environment_id: &str,
    day: &str,
    dimension: &str,
    value: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"sonde:dimension-rollup:v1\0");
    for part in [application_id, environment_id, day, dimension, value] {
        hasher.update(part.as_bytes());
        hasher.update(b"\0");
    }
    format!("dd_{}", hex::encode(hasher.finalize()))
}

fn day_for_timestamp(timestamp: i64) -> Option<String> {
    chrono::DateTime::from_timestamp_millis(timestamp)
        .map(|value| value.format("%Y-%m-%d").to_string())
}

fn day_bounds(day: &str) -> Result<(i64, i64), DbErr> {
    let date = NaiveDate::parse_from_str(day, "%Y-%m-%d")
        .map_err(|error| DbErr::Custom(error.to_string()))?;
    let start = date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| DbErr::Custom("invalid dimension rollup day".into()))?
        .and_utc()
        .timestamp_millis();
    Ok((start, start.saturating_add(86_400_000)))
}

fn positive_u64(value: i64) -> u64 {
    value.max(0) as u64
}

fn saturating_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::{os_build_name, os_family_name};

    #[test]
    fn normalizes_operating_system_dimensions() {
        assert_eq!(os_family_name("Windows 11 Build 26100"), "Windows");
        assert_eq!(os_family_name("Ubuntu 24.04 Linux x86_64"), "Linux");
        assert_eq!(os_build_name("Windows 11 Build 26100"), "Win 11 (26100)");
        assert_eq!(os_build_name("Darwin 25.0"), "macOS 25.0");
    }
}
