use std::collections::{BTreeMap, HashSet};

use sea_orm::{
    ConnectionTrait, DbBackend, DbErr,
    sea_query::{Alias, Expr, ExprTrait, Func, Order, Query},
};
use sha2::{Digest, Sha256};

use super::{query::insert_batch_ignore_conflicts, telemetry::TelemetryScope};

pub const MAX_CONTIGUOUS_ACTIVITY_MILLIS: i64 = 30 * 60_000;
const HOUR_MILLIS: i64 = 60 * 60_000;
const DAY_MILLIS: i64 = 24 * HOUR_MILLIS;

#[derive(Clone, Debug)]
pub struct DailyActiveDevices {
    pub day: String,
    pub devices: u64,
    pub active_millis: u64,
}

#[derive(Clone, Debug)]
pub struct DeviceGrowthBucket {
    pub bucket: String,
    pub new_devices: u64,
    pub cumulative_devices: u64,
    pub active_devices: u64,
}

#[derive(Clone, Debug)]
pub struct ActivityBucket {
    pub bucket: String,
    pub active_devices: u64,
    pub active_millis: u64,
    pub cumulative_active_millis: u64,
}

#[derive(Clone, Copy)]
enum ActivityGranularity {
    Hour,
    Day,
    Month,
}

/// Record an authoritative live device observation.
///
/// Only a contiguous server-observed gap of at most 30 minutes contributes active time. The gap is
/// split across UTC day/hour boundaries so every bucket receives the exact interval it owns.
pub async fn record(
    database: &impl ConnectionTrait,
    scope: &TelemetryScope,
    device_hash: &str,
    previous_seen_at: i64,
    received_at: i64,
) -> Result<(), DbErr> {
    touch_request_bucket(
        database,
        "telemetry_device_activity_days",
        "day",
        "sonde:device-activity-day\0",
        &day_for_timestamp(received_at),
        scope,
        device_hash,
        received_at,
    )
    .await?;
    touch_request_bucket(
        database,
        "telemetry_device_activity_hours",
        "hour",
        "sonde:device-activity-hour\0",
        &hour_for_timestamp(received_at),
        scope,
        device_hash,
        received_at,
    )
    .await?;

    let gap = received_at.saturating_sub(previous_seen_at);
    if gap <= 0 || gap > MAX_CONTIGUOUS_ACTIVITY_MILLIS {
        return Ok(());
    }

    add_interval_slices(
        database,
        "telemetry_device_activity_days",
        "day",
        "sonde:device-activity-day\0",
        DAY_MILLIS,
        scope,
        device_hash,
        previous_seen_at,
        received_at,
        day_for_timestamp,
    )
    .await?;
    add_interval_slices(
        database,
        "telemetry_device_activity_hours",
        "hour",
        "sonde:device-activity-hour\0",
        HOUR_MILLIS,
        scope,
        device_hash,
        previous_seen_at,
        received_at,
        hour_for_timestamp,
    )
    .await
}

/// Project a historical observation into active-device presence without fabricating online time.
/// Historical events do not prove that a device stayed online between two event timestamps.
pub async fn record_historical_presence(
    database: &impl ConnectionTrait,
    scope: &TelemetryScope,
    device_hash: &str,
    timestamp: i64,
) -> Result<(), DbErr> {
    touch_presence_bucket(
        database,
        "telemetry_device_activity_days",
        "day",
        "sonde:device-activity-day\0",
        &day_for_timestamp(timestamp),
        scope,
        device_hash,
        timestamp,
    )
    .await?;
    touch_presence_bucket(
        database,
        "telemetry_device_activity_hours",
        "hour",
        "sonde:device-activity-hour\0",
        &hour_for_timestamp(timestamp),
        scope,
        device_hash,
        timestamp,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn touch_request_bucket(
    database: &impl ConnectionTrait,
    table: &str,
    bucket_column: &str,
    id_context: &str,
    bucket: &str,
    scope: &TelemetryScope,
    device_hash: &str,
    received_at: i64,
) -> Result<(), DbErr> {
    touch_bucket(
        database,
        table,
        bucket_column,
        id_context,
        bucket,
        scope,
        device_hash,
        received_at,
        1,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn touch_presence_bucket(
    database: &impl ConnectionTrait,
    table: &str,
    bucket_column: &str,
    id_context: &str,
    bucket: &str,
    scope: &TelemetryScope,
    device_hash: &str,
    timestamp: i64,
) -> Result<(), DbErr> {
    touch_bucket(
        database,
        table,
        bucket_column,
        id_context,
        bucket,
        scope,
        device_hash,
        timestamp,
        0,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn touch_bucket(
    database: &impl ConnectionTrait,
    table: &str,
    bucket_column: &str,
    id_context: &str,
    bucket: &str,
    scope: &TelemetryScope,
    device_hash: &str,
    timestamp: i64,
    request_increment: i64,
) -> Result<(), DbErr> {
    let id = activity_id(
        id_context,
        &scope.application_id,
        &scope.environment_id,
        device_hash,
        bucket,
    );
    let inserted = insert_batch_ignore_conflicts(
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
            request_increment.into(),
            timestamp.into(),
        ]],
        "id",
        "id",
    )
    .await?;
    if inserted > 0 {
        return Ok(());
    }

    let mut update = Query::update();
    update
        .table(Alias::new(table))
        .value(
            Alias::new("first_seen_at"),
            Expr::cust_with_values(
                "CASE WHEN first_seen_at > ? THEN ? ELSE first_seen_at END",
                [timestamp, timestamp],
            ),
        )
        .value(
            Alias::new("last_seen_at"),
            Expr::cust_with_values(
                "CASE WHEN last_seen_at < ? THEN ? ELSE last_seen_at END",
                [timestamp, timestamp],
            ),
        )
        .value(Alias::new("updated_at"), timestamp)
        .and_where(Expr::col(Alias::new("id")).eq(id));
    if request_increment > 0 {
        update.value(
            Alias::new("request_count"),
            Expr::col(Alias::new("request_count")).add(request_increment),
        );
    }
    database.execute(&update.to_owned()).await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn add_interval_slices(
    database: &impl ConnectionTrait,
    table: &str,
    bucket_column: &str,
    id_context: &str,
    bucket_millis: i64,
    scope: &TelemetryScope,
    device_hash: &str,
    start: i64,
    end: i64,
    bucket_name: fn(i64) -> String,
) -> Result<(), DbErr> {
    let mut cursor = start;
    while cursor < end {
        let boundary = cursor
            .div_euclid(bucket_millis)
            .saturating_add(1)
            .saturating_mul(bucket_millis);
        let slice_end = std::cmp::min(boundary, end);
        if slice_end <= cursor {
            return Err(DbErr::Custom("invalid device activity bucket boundary".into()));
        }
        add_activity_slice(
            database,
            table,
            bucket_column,
            id_context,
            &bucket_name(cursor),
            scope,
            device_hash,
            cursor,
            slice_end,
        )
        .await?;
        cursor = slice_end;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn add_activity_slice(
    database: &impl ConnectionTrait,
    table: &str,
    bucket_column: &str,
    id_context: &str,
    bucket: &str,
    scope: &TelemetryScope,
    device_hash: &str,
    slice_start: i64,
    slice_end: i64,
) -> Result<(), DbErr> {
    let active_delta = slice_end.saturating_sub(slice_start);
    if active_delta <= 0 {
        return Ok(());
    }
    let id = activity_id(
        id_context,
        &scope.application_id,
        &scope.environment_id,
        device_hash,
        bucket,
    );
    let inserted = insert_batch_ignore_conflicts(
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
            slice_start.into(),
            slice_end.into(),
            active_delta.into(),
            0_i64.into(),
            slice_end.into(),
        ]],
        "id",
        "id",
    )
    .await?;
    if inserted > 0 {
        return Ok(());
    }

    let update = Query::update()
        .table(Alias::new(table))
        .value(
            Alias::new("first_seen_at"),
            Expr::cust_with_values(
                "CASE WHEN first_seen_at > ? THEN ? ELSE first_seen_at END",
                [slice_start, slice_start],
            ),
        )
        .value(
            Alias::new("last_seen_at"),
            Expr::cust_with_values(
                "CASE WHEN last_seen_at < ? THEN ? ELSE last_seen_at END",
                [slice_end, slice_end],
            ),
        )
        .value(
            Alias::new("active_millis"),
            Expr::col(Alias::new("active_millis")).add(active_delta),
        )
        .value(Alias::new("updated_at"), slice_end)
        .and_where(Expr::col(Alias::new("id")).eq(id))
        .to_owned();
    database.execute(&update).await?;
    Ok(())
}

pub async fn unique_devices(
    database: &impl ConnectionTrait,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since: Option<i64>,
    until: Option<i64>,
) -> Result<u64, DbErr> {
    if matches!((since, until), (Some(start), Some(end)) if end <= start) {
        return Ok(0);
    }
    let mut query = Query::select();
    query
        .expr_as(
            Expr::cust("COUNT(DISTINCT device_hash)"),
            Alias::new("count"),
        )
        .from(Alias::new("telemetry_device_activity_days"));
    apply_scope_and_window(
        &mut query,
        application_id,
        environment_id,
        since,
        until,
    );
    positive_count(database, query).await
}

pub async fn new_devices(
    database: &impl ConnectionTrait,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since: i64,
    until: Option<i64>,
) -> Result<u64, DbErr> {
    let mut query = Query::select();
    query
        .expr_as(Func::count(Expr::col(Alias::new("id"))), Alias::new("count"))
        .from(Alias::new("telemetry_devices"))
        .and_where(Expr::col(Alias::new("first_seen_at")).gte(since));
    if let Some(until) = until {
        query.and_where(Expr::col(Alias::new("first_seen_at")).lt(until));
    }
    apply_scope(&mut query, application_id, environment_id);
    positive_count(database, query).await
}

pub async fn total_devices(
    database: &impl ConnectionTrait,
    application_id: Option<&str>,
    environment_id: Option<&str>,
) -> Result<u64, DbErr> {
    let mut query = Query::select();
    query
        .expr_as(Func::count(Expr::col(Alias::new("id"))), Alias::new("count"))
        .from(Alias::new("telemetry_devices"));
    apply_scope(&mut query, application_id, environment_id);
    positive_count(database, query).await
}

pub async fn total_active_millis(
    database: &impl ConnectionTrait,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since: Option<i64>,
) -> Result<u64, DbErr> {
    let mut query = Query::select();
    query
        .column(Alias::new("active_millis"))
        .from(Alias::new("telemetry_device_activity_days"));
    apply_scope(&mut query, application_id, environment_id);
    if let Some(since) = since {
        query.and_where(Expr::col(Alias::new("last_seen_at")).gte(since));
    }
    let mut total = 0_u64;
    for row in database.query_all(&query.to_owned()).await? {
        total = total.saturating_add(nonnegative(
            row.try_get::<i64>("", "active_millis").unwrap_or(0),
        ));
    }
    Ok(total)
}

pub async fn growth_timeline(
    database: &impl ConnectionTrait,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since: Option<i64>,
    days: Option<u32>,
) -> Result<Vec<DeviceGrowthBucket>, DbErr> {
    let granularity = granularity(days);
    let active = active_buckets(database, application_id, environment_id, since, granularity).await?;
    let new = new_device_buckets(database, application_id, environment_id, since, granularity).await?;
    let baseline = match since {
        Some(since) => devices_before(database, application_id, environment_id, since).await?,
        None => 0,
    };

    let mut buckets = BTreeMap::<String, (u64, u64)>::new();
    for (bucket, active_devices) in active {
        buckets.entry(bucket).or_default().0 = active_devices;
    }
    for (bucket, new_devices) in new {
        buckets.entry(bucket).or_default().1 = new_devices;
    }

    let mut cumulative = baseline;
    Ok(buckets
        .into_iter()
        .map(|(bucket, (active_devices, new_devices))| {
            cumulative = cumulative.saturating_add(new_devices);
            DeviceGrowthBucket {
                bucket,
                new_devices,
                cumulative_devices: cumulative,
                active_devices,
            }
        })
        .collect())
}

pub async fn activity_timeline(
    database: &impl ConnectionTrait,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since: Option<i64>,
    days: Option<u32>,
) -> Result<Vec<ActivityBucket>, DbErr> {
    let granularity = granularity(days);
    let rows = activity_buckets(database, application_id, environment_id, since, granularity).await?;
    let mut cumulative = 0_u64;
    Ok(rows
        .into_iter()
        .map(|(bucket, (active_devices, active_millis))| {
            cumulative = cumulative.saturating_add(active_millis);
            ActivityBucket {
                bucket,
                active_devices,
                active_millis,
                cumulative_active_millis: cumulative,
            }
        })
        .collect())
}

pub async fn daily_activity(
    database: &impl ConnectionTrait,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since: Option<i64>,
) -> Result<Vec<DailyActiveDevices>, DbErr> {
    let mut query = Query::select();
    query
        .columns(["day", "active_millis"].map(Alias::new))
        .from(Alias::new("telemetry_device_activity_days"))
        .order_by(Alias::new("day"), Order::Asc);
    apply_scope(&mut query, application_id, environment_id);
    if let Some(since) = since {
        query.and_where(Expr::col(Alias::new("last_seen_at")).gte(since));
    }

    let mut buckets = BTreeMap::<String, (u64, u64)>::new();
    for row in database.query_all(&query.to_owned()).await? {
        let day: String = row.try_get("", "day")?;
        let active_millis = nonnegative(row.try_get::<i64>("", "active_millis").unwrap_or(0));
        buckets
            .entry(day)
            .and_modify(|value| {
                value.0 = value.0.saturating_add(1);
                value.1 = value.1.saturating_add(active_millis);
            })
            .or_insert((1, active_millis));
    }
    Ok(buckets
        .into_iter()
        .map(|(day, (devices, active_millis))| DailyActiveDevices {
            day,
            devices,
            active_millis,
        })
        .collect())
}

async fn active_buckets(
    database: &impl ConnectionTrait,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since: Option<i64>,
    granularity: ActivityGranularity,
) -> Result<BTreeMap<String, u64>, DbErr> {
    Ok(activity_buckets(database, application_id, environment_id, since, granularity)
        .await?
        .into_iter()
        .map(|(bucket, (devices, _))| (bucket, devices))
        .collect())
}

async fn activity_buckets(
    database: &impl ConnectionTrait,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since: Option<i64>,
    granularity: ActivityGranularity,
) -> Result<BTreeMap<String, (u64, u64)>, DbErr> {
    let (table, bucket_column) = match granularity {
        ActivityGranularity::Hour => ("telemetry_device_activity_hours", "hour"),
        ActivityGranularity::Day | ActivityGranularity::Month => {
            ("telemetry_device_activity_days", "day")
        }
    };
    let mut query = Query::select();
    query
        .columns([bucket_column, "device_hash", "active_millis"].map(Alias::new))
        .from(Alias::new(table))
        .order_by(Alias::new(bucket_column), Order::Asc);
    apply_scope(&mut query, application_id, environment_id);
    if let Some(since) = since {
        query.and_where(Expr::col(Alias::new("last_seen_at")).gte(since));
    }

    let mut buckets = BTreeMap::<String, (HashSet<String>, u64)>::new();
    for row in database.query_all(&query.to_owned()).await? {
        let raw_bucket: String = row.try_get("", bucket_column)?;
        let bucket = match granularity {
            ActivityGranularity::Month => raw_bucket
                .get(..7)
                .ok_or_else(|| DbErr::Custom("invalid device activity day bucket".into()))?
                .to_owned(),
            _ => raw_bucket,
        };
        let device_hash: String = row.try_get("", "device_hash")?;
        let active_millis = nonnegative(row.try_get::<i64>("", "active_millis").unwrap_or(0));
        buckets
            .entry(bucket)
            .and_modify(|value| {
                value.0.insert(device_hash.clone());
                value.1 = value.1.saturating_add(active_millis);
            })
            .or_insert_with(|| {
                let mut devices = HashSet::new();
                devices.insert(device_hash);
                (devices, active_millis)
            });
    }
    Ok(buckets
        .into_iter()
        .map(|(bucket, (devices, active_millis))| {
            (
                bucket,
                (u64::try_from(devices.len()).unwrap_or(u64::MAX), active_millis),
            )
        })
        .collect())
}

async fn new_device_buckets(
    database: &impl ConnectionTrait,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since: Option<i64>,
    granularity: ActivityGranularity,
) -> Result<BTreeMap<String, u64>, DbErr> {
    let bucket_expr = first_seen_bucket_expr(database.get_database_backend(), granularity)?;
    let mut query = Query::select();
    query
        .expr_as(Expr::cust(bucket_expr), Alias::new("bucket"))
        .expr_as(Func::count(Expr::col(Alias::new("id"))), Alias::new("devices"))
        .from(Alias::new("telemetry_devices"))
        .and_where(Expr::col(Alias::new("first_seen_at")).is_not_null())
        .group_by_col(Alias::new("bucket"))
        .order_by(Alias::new("bucket"), Order::Asc);
    apply_scope(&mut query, application_id, environment_id);
    if let Some(since) = since {
        query.and_where(Expr::col(Alias::new("first_seen_at")).gte(since));
    }

    let mut result = BTreeMap::new();
    for row in database.query_all(&query.to_owned()).await? {
        result.insert(
            row.try_get("", "bucket")?,
            nonnegative(row.try_get::<i64>("", "devices").unwrap_or(0)),
        );
    }
    Ok(result)
}

async fn devices_before(
    database: &impl ConnectionTrait,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    before: i64,
) -> Result<u64, DbErr> {
    let mut query = Query::select();
    query
        .expr_as(Func::count(Expr::col(Alias::new("id"))), Alias::new("count"))
        .from(Alias::new("telemetry_devices"))
        .and_where(Expr::col(Alias::new("first_seen_at")).lt(before));
    apply_scope(&mut query, application_id, environment_id);
    positive_count(database, query).await
}

fn granularity(days: Option<u32>) -> ActivityGranularity {
    match days {
        Some(1) => ActivityGranularity::Hour,
        Some(365) | None => ActivityGranularity::Month,
        _ => ActivityGranularity::Day,
    }
}

fn first_seen_bucket_expr(
    backend: DbBackend,
    granularity: ActivityGranularity,
) -> Result<String, DbErr> {
    let expression = match (backend, granularity) {
        (DbBackend::Postgres, ActivityGranularity::Hour) => {
            "to_char(to_timestamp(first_seen_at / 1000.0), 'YYYY-MM-DD HH24:00')"
        }
        (DbBackend::Postgres, ActivityGranularity::Day) => {
            "to_char(to_timestamp(first_seen_at / 1000.0), 'YYYY-MM-DD')"
        }
        (DbBackend::Postgres, ActivityGranularity::Month) => {
            "to_char(to_timestamp(first_seen_at / 1000.0), 'YYYY-MM')"
        }
        (DbBackend::MySql, ActivityGranularity::Hour) => {
            "DATE_FORMAT(FROM_UNIXTIME(first_seen_at / 1000), '%Y-%m-%d %H:00')"
        }
        (DbBackend::MySql, ActivityGranularity::Day) => {
            "DATE_FORMAT(FROM_UNIXTIME(first_seen_at / 1000), '%Y-%m-%d')"
        }
        (DbBackend::MySql, ActivityGranularity::Month) => {
            "DATE_FORMAT(FROM_UNIXTIME(first_seen_at / 1000), '%Y-%m')"
        }
        (DbBackend::Sqlite, ActivityGranularity::Hour) => {
            "strftime('%Y-%m-%d %H:00', first_seen_at / 1000, 'unixepoch')"
        }
        (DbBackend::Sqlite, ActivityGranularity::Day) => {
            "strftime('%Y-%m-%d', first_seen_at / 1000, 'unixepoch')"
        }
        (DbBackend::Sqlite, ActivityGranularity::Month) => {
            "strftime('%Y-%m', first_seen_at / 1000, 'unixepoch')"
        }
        _ => return Err(DbErr::Custom("unsupported database backend for device activity".into())),
    };
    Ok(expression.to_owned())
}

async fn positive_count(
    database: &impl ConnectionTrait,
    query: sea_orm::sea_query::SelectStatement,
) -> Result<u64, DbErr> {
    let count = database
        .query_one(&query)
        .await?
        .and_then(|row| row.try_get::<i64>("", "count").ok())
        .unwrap_or(0);
    u64::try_from(count).map_err(|_| DbErr::Custom("negative device count".into()))
}

fn apply_scope(
    query: &mut sea_orm::sea_query::SelectStatement,
    application_id: Option<&str>,
    environment_id: Option<&str>,
) {
    if let Some(application_id) = application_id {
        query.and_where(Expr::col(Alias::new("application_id")).eq(application_id));
    }
    if let Some(environment_id) = environment_id {
        query.and_where(Expr::col(Alias::new("environment_id")).eq(environment_id));
    }
}

fn apply_scope_and_window(
    query: &mut sea_orm::sea_query::SelectStatement,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since: Option<i64>,
    until: Option<i64>,
) {
    apply_scope(query, application_id, environment_id);
    if let Some(since) = since {
        query.and_where(Expr::col(Alias::new("last_seen_at")).gte(since));
    }
    if let Some(until) = until {
        query.and_where(Expr::col(Alias::new("first_seen_at")).lt(until));
    }
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

fn nonnegative(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{activity_id, DAY_MILLIS, HOUR_MILLIS};

    #[test]
    fn activity_ids_are_scope_and_bucket_bound() {
        let one = activity_id(
            "sonde:device-activity-day\0",
            "app",
            "prod",
            "device",
            "2026-09-01",
        );
        let two = activity_id(
            "sonde:device-activity-day\0",
            "app",
            "prod",
            "device",
            "2026-09-02",
        );
        assert_eq!(one.len(), 64);
        assert_ne!(one, two);
    }

    #[test]
    fn utc_bucket_widths_are_exact() {
        assert_eq!(HOUR_MILLIS, 3_600_000);
        assert_eq!(DAY_MILLIS, 86_400_000);
    }
}
