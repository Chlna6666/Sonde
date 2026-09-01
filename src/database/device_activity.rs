use std::collections::BTreeMap;

use sea_orm::{
    ConnectionTrait, DbBackend, DbErr,
    sea_query::{Alias, Expr, ExprTrait, Func, Order, Query},
};
use sha2::{Digest, Sha256};

use super::{query::insert_batch_ignore_conflicts, telemetry::TelemetryScope};

const MAX_CONTIGUOUS_ACTIVITY_MILLIS: i64 = 30 * 60_000;

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

#[derive(Clone, Copy)]
enum ActivityGranularity {
    Hour,
    Day,
    Month,
}

pub async fn record(
    database: &impl ConnectionTrait,
    scope: &TelemetryScope,
    device_hash: &str,
    previous_seen_at: i64,
    received_at: i64,
) -> Result<(), DbErr> {
    record_bucket(
        database,
        "telemetry_device_activity_days",
        "day",
        "sonde:device-activity-day\0",
        &day_for_timestamp(previous_seen_at),
        &day_for_timestamp(received_at),
        scope,
        device_hash,
        previous_seen_at,
        received_at,
    )
    .await?;
    record_bucket(
        database,
        "telemetry_device_activity_hours",
        "hour",
        "sonde:device-activity-hour\0",
        &hour_for_timestamp(previous_seen_at),
        &hour_for_timestamp(received_at),
        scope,
        device_hash,
        previous_seen_at,
        received_at,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn record_bucket(
    database: &impl ConnectionTrait,
    table: &str,
    bucket_column: &str,
    id_context: &str,
    previous_bucket: &str,
    current_bucket: &str,
    scope: &TelemetryScope,
    device_hash: &str,
    previous_seen_at: i64,
    received_at: i64,
) -> Result<(), DbErr> {
    let id = activity_id(
        id_context,
        &scope.application_id,
        &scope.environment_id,
        device_hash,
        current_bucket,
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
            current_bucket.to_owned().into(),
            received_at.into(),
            received_at.into(),
            0_i64.into(),
            1_i64.into(),
            received_at.into(),
        ]],
        "id",
        "id",
    )
    .await?;
    if inserted > 0 {
        return Ok(());
    }

    let gap = received_at.saturating_sub(previous_seen_at);
    let active_delta = if previous_bucket == current_bucket
        && gap > 0
        && gap <= MAX_CONTIGUOUS_ACTIVITY_MILLIS
    {
        gap
    } else {
        0
    };
    let update = Query::update()
        .table(Alias::new(table))
        .value(
            Alias::new("last_seen_at"),
            std::cmp::max(previous_seen_at, received_at),
        )
        .value(
            Alias::new("active_millis"),
            Expr::col(Alias::new("active_millis")).add(active_delta),
        )
        .value(
            Alias::new("request_count"),
            Expr::col(Alias::new("request_count")).add(1_i64),
        )
        .value(Alias::new("updated_at"), received_at)
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

pub async fn growth_timeline(
    database: &impl ConnectionTrait,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since: Option<i64>,
    days: Option<u32>,
) -> Result<Vec<DeviceGrowthBucket>, DbErr> {
    let granularity = match days {
        Some(1) => ActivityGranularity::Hour,
        Some(365) | None => ActivityGranularity::Month,
        _ => ActivityGranularity::Day,
    };
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

pub async fn daily_activity(
    database: &impl ConnectionTrait,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since: Option<i64>,
) -> Result<Vec<DailyActiveDevices>, DbErr> {
    let mut query = Query::select();
    query
        .column(Alias::new("day"))
        .expr_as(Func::count(Expr::col(Alias::new("id"))), Alias::new("devices"))
        .expr_as(
            Func::sum(Expr::col(Alias::new("active_millis"))),
            Alias::new("active_millis"),
        )
        .from(Alias::new("telemetry_device_activity_days"))
        .group_by_col(Alias::new("day"))
        .order_by(Alias::new("day"), Order::Asc);
    apply_scope(&mut query, application_id, environment_id);
    if let Some(since) = since {
        query.and_where(Expr::col(Alias::new("last_seen_at")).gte(since));
    }

    database
        .query_all(&query.to_owned())
        .await?
        .into_iter()
        .map(|row| {
            Ok(DailyActiveDevices {
                day: row.try_get("", "day")?,
                devices: nonnegative(row.try_get::<i64>("", "devices").unwrap_or(0)),
                active_millis: nonnegative(
                    row.try_get::<i64>("", "active_millis").unwrap_or(0),
                ),
            })
        })
        .collect()
}

async fn active_buckets(
    database: &impl ConnectionTrait,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since: Option<i64>,
    granularity: ActivityGranularity,
) -> Result<BTreeMap<String, u64>, DbErr> {
    if matches!(granularity, ActivityGranularity::Month) {
        let mut query = Query::select();
        query
            .expr_as(Expr::cust("SUBSTR(day, 1, 7)"), Alias::new("bucket"))
            .expr_as(
                Expr::cust("COUNT(DISTINCT device_hash)"),
                Alias::new("devices"),
            )
            .from(Alias::new("telemetry_device_activity_days"))
            .group_by_col(Alias::new("bucket"))
            .order_by(Alias::new("bucket"), Order::Asc);
        apply_scope(&mut query, application_id, environment_id);
        if let Some(since) = since {
            query.and_where(Expr::col(Alias::new("last_seen_at")).gte(since));
        }
        let mut result = BTreeMap::new();
        for row in database.query_all(&query.to_owned()).await? {
            result.insert(
                row.try_get("", "bucket")?,
                nonnegative(row.try_get::<i64>("", "devices").unwrap_or(0)),
            );
        }
        return Ok(result);
    }

    let (table, column) = match granularity {
        ActivityGranularity::Hour => ("telemetry_device_activity_hours", "hour"),
        ActivityGranularity::Day => ("telemetry_device_activity_days", "day"),
        ActivityGranularity::Month => unreachable!(),
    };
    let mut query = Query::select();
    query
        .column(Alias::new(column))
        .expr_as(Func::count(Expr::col(Alias::new("id"))), Alias::new("devices"))
        .from(Alias::new(table))
        .group_by_col(Alias::new(column))
        .order_by(Alias::new(column), Order::Asc);
    apply_scope(&mut query, application_id, environment_id);
    if let Some(since) = since {
        query.and_where(Expr::col(Alias::new("last_seen_at")).gte(since));
    }

    let mut result = BTreeMap::new();
    for row in database.query_all(&query.to_owned()).await? {
        result.insert(
            row.try_get("", column)?,
            nonnegative(row.try_get::<i64>("", "devices").unwrap_or(0)),
        );
    }
    Ok(result)
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
    use super::activity_id;

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
}
