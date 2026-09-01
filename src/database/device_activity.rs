use sea_orm::{
    ConnectionTrait, DbErr,
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

pub async fn record(
    database: &impl ConnectionTrait,
    scope: &TelemetryScope,
    device_hash: &str,
    previous_seen_at: i64,
    received_at: i64,
) -> Result<(), DbErr> {
    let day = day_for_timestamp(received_at);
    let id = activity_id(
        &scope.application_id,
        &scope.environment_id,
        device_hash,
        &day,
    );
    let inserted = insert_batch_ignore_conflicts(
        database,
        "telemetry_device_activity_days",
        &[
            "id",
            "application_id",
            "environment_id",
            "device_hash",
            "day",
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
            day.clone().into(),
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

    let previous_day = day_for_timestamp(previous_seen_at);
    let gap = received_at.saturating_sub(previous_seen_at);
    let active_delta = if previous_day == day
        && gap > 0
        && gap <= MAX_CONTIGUOUS_ACTIVITY_MILLIS
    {
        gap
    } else {
        0
    };
    let last_seen_at = if previous_day == day {
        std::cmp::max(previous_seen_at, received_at)
    } else {
        received_at
    };
    let update = Query::update()
        .table(Alias::new("telemetry_device_activity_days"))
        .value(Alias::new("last_seen_at"), last_seen_at)
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
    let count = database
        .query_one(&query.to_owned())
        .await?
        .and_then(|row| row.try_get::<i64>("", "count").ok())
        .unwrap_or(0);
    u64::try_from(count).map_err(|_| DbErr::Custom("negative unique device count".into()))
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
    if let Some(application_id) = application_id {
        query.and_where(Expr::col(Alias::new("application_id")).eq(application_id));
    }
    if let Some(environment_id) = environment_id {
        query.and_where(Expr::col(Alias::new("environment_id")).eq(environment_id));
    }
    let count = database
        .query_one(&query.to_owned())
        .await?
        .and_then(|row| row.try_get::<i64>("", "count").ok())
        .unwrap_or(0);
    u64::try_from(count).map_err(|_| DbErr::Custom("negative new device count".into()))
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
    if let Some(application_id) = application_id {
        query.and_where(Expr::col(Alias::new("application_id")).eq(application_id));
    }
    if let Some(environment_id) = environment_id {
        query.and_where(Expr::col(Alias::new("environment_id")).eq(environment_id));
    }
    if let Some(since) = since {
        query.and_where(Expr::col(Alias::new("last_seen_at")).gte(since));
    }

    database
        .query_all(&query.to_owned())
        .await?
        .into_iter()
        .map(|row| {
            let devices = row.try_get::<i64>("", "devices").unwrap_or(0);
            let active_millis = row.try_get::<i64>("", "active_millis").unwrap_or(0);
            Ok(DailyActiveDevices {
                day: row.try_get("", "day")?,
                devices: u64::try_from(std::cmp::max(devices, 0)).unwrap_or(0),
                active_millis: u64::try_from(std::cmp::max(active_millis, 0)).unwrap_or(0),
            })
        })
        .collect()
}

fn apply_scope_and_window(
    query: &mut sea_orm::sea_query::SelectStatement,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since: Option<i64>,
    until: Option<i64>,
) {
    if let Some(application_id) = application_id {
        query.and_where(Expr::col(Alias::new("application_id")).eq(application_id));
    }
    if let Some(environment_id) = environment_id {
        query.and_where(Expr::col(Alias::new("environment_id")).eq(environment_id));
    }
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

fn activity_id(application_id: &str, environment_id: &str, device_hash: &str, day: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"sonde:device-activity-day\0");
    for value in [application_id, environment_id, device_hash, day] {
        hasher.update(value.as_bytes());
        hasher.update(b"\0");
    }
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::activity_id;

    #[test]
    fn activity_ids_are_scope_and_day_bound() {
        let one = activity_id("app", "prod", "device", "2026-09-01");
        let two = activity_id("app", "prod", "device", "2026-09-02");
        assert_eq!(one.len(), 64);
        assert_ne!(one, two);
    }
}
