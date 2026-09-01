use std::collections::BTreeMap;

use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr,
    sea_query::{Alias, Expr, ExprTrait, Query},
};

use super::{device_activity, device_session};

#[derive(Clone, Debug, Default)]
pub struct ActivitySummary {
    pub active_millis: u64,
    pub lifetime_active_millis: u64,
    pub sessions: u64,
    pub lifetime_sessions: u64,
    pub measured_devices: u64,
    pub measurement_coverage_pct: f64,
    pub average_session_millis: u64,
    pub average_active_millis_per_device: u64,
    pub stickiness_pct: f64,
}

#[derive(Clone, Debug)]
pub struct ActivityTrendPoint {
    pub bucket: String,
    pub active_users: u64,
    pub active_millis: u64,
    pub sessions: u64,
    pub average_session_millis: u64,
    pub cumulative_active_millis: u64,
    pub cumulative_sessions: u64,
    pub lifetime_cumulative_active_millis: u64,
    pub lifetime_cumulative_sessions: u64,
}

#[derive(Clone, Debug, Default)]
pub struct ActivityStats {
    pub summary: ActivitySummary,
    pub trend: Vec<ActivityTrendPoint>,
}

pub async fn query(
    database: &DatabaseConnection,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since: Option<i64>,
    days: Option<u32>,
) -> Result<ActivityStats, DbErr> {
    let now = chrono::Utc::now().timestamp_millis();
    let active_millis = device_activity::total_active_millis(
        database,
        application_id,
        environment_id,
        since,
    )
    .await?;
    let lifetime_active_millis = device_activity::total_active_millis(
        database,
        application_id,
        environment_id,
        None,
    )
    .await?;
    let sessions = device_session::summary(
        database,
        application_id,
        environment_id,
        since,
        None,
    )
    .await?;
    let lifetime_sessions = device_session::summary(
        database,
        application_id,
        environment_id,
        None,
        None,
    )
    .await?;
    let active_devices = device_activity::unique_devices(
        database,
        application_id,
        environment_id,
        since,
        None,
    )
    .await?;
    let measured_devices = measured_devices(
        database,
        application_id,
        environment_id,
        since,
    )
    .await?;
    let dau = device_activity::unique_devices(
        database,
        application_id,
        environment_id,
        Some(now.saturating_sub(86_400_000)),
        None,
    )
    .await?;
    let mau = device_activity::unique_devices(
        database,
        application_id,
        environment_id,
        Some(now.saturating_sub(30 * 86_400_000)),
        None,
    )
    .await?;

    let activity = device_activity::activity_timeline(
        database,
        application_id,
        environment_id,
        since,
        days,
    )
    .await?;
    let session_buckets = device_session::buckets(
        database,
        application_id,
        environment_id,
        since,
        days,
    )
    .await?;

    let mut buckets = BTreeMap::<String, ActivityTrendPoint>::new();
    for point in activity {
        buckets.insert(
            point.bucket.clone(),
            ActivityTrendPoint {
                bucket: point.bucket,
                active_users: point.active_devices,
                active_millis: point.active_millis,
                sessions: 0,
                average_session_millis: 0,
                cumulative_active_millis: 0,
                cumulative_sessions: 0,
                lifetime_cumulative_active_millis: 0,
                lifetime_cumulative_sessions: 0,
            },
        );
    }
    for point in session_buckets {
        let average_session_millis = if point.sessions == 0 {
            0
        } else {
            point.active_millis / point.sessions
        };
        buckets
            .entry(point.bucket.clone())
            .and_modify(|bucket| {
                bucket.sessions = point.sessions;
                bucket.average_session_millis = average_session_millis;
            })
            .or_insert(ActivityTrendPoint {
                bucket: point.bucket,
                active_users: 0,
                active_millis: 0,
                sessions: point.sessions,
                average_session_millis,
                cumulative_active_millis: 0,
                cumulative_sessions: 0,
                lifetime_cumulative_active_millis: 0,
                lifetime_cumulative_sessions: 0,
            });
    }

    let active_baseline = lifetime_active_millis.saturating_sub(active_millis);
    let session_baseline = lifetime_sessions
        .total_sessions
        .saturating_sub(sessions.total_sessions);
    let mut cumulative_active_millis = 0_u64;
    let mut cumulative_sessions = 0_u64;
    let mut lifetime_cumulative_active_millis = active_baseline;
    let mut lifetime_cumulative_sessions = session_baseline;
    for point in buckets.values_mut() {
        cumulative_active_millis = cumulative_active_millis.saturating_add(point.active_millis);
        cumulative_sessions = cumulative_sessions.saturating_add(point.sessions);
        lifetime_cumulative_active_millis =
            lifetime_cumulative_active_millis.saturating_add(point.active_millis);
        lifetime_cumulative_sessions = lifetime_cumulative_sessions.saturating_add(point.sessions);
        point.cumulative_active_millis = cumulative_active_millis;
        point.cumulative_sessions = cumulative_sessions;
        point.lifetime_cumulative_active_millis = lifetime_cumulative_active_millis;
        point.lifetime_cumulative_sessions = lifetime_cumulative_sessions;
    }

    Ok(ActivityStats {
        summary: ActivitySummary {
            active_millis,
            lifetime_active_millis,
            sessions: sessions.total_sessions,
            lifetime_sessions: lifetime_sessions.total_sessions,
            measured_devices,
            measurement_coverage_pct: percentage(measured_devices, active_devices),
            average_session_millis: sessions.average_session_millis,
            average_active_millis_per_device: if measured_devices == 0 {
                0
            } else {
                active_millis / measured_devices
            },
            stickiness_pct: percentage(dau, mau),
        },
        trend: buckets.into_values().collect(),
    })
}

async fn measured_devices(
    database: &DatabaseConnection,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since: Option<i64>,
) -> Result<u64, DbErr> {
    let mut query = Query::select();
    query
        .expr_as(
            Expr::cust("COUNT(DISTINCT device_hash)"),
            Alias::new("count"),
        )
        .from(Alias::new("telemetry_device_activity_days"))
        .and_where(Expr::col(Alias::new("active_millis")).gt(0_i64));
    if let Some(application_id) = application_id {
        query.and_where(Expr::col(Alias::new("application_id")).eq(application_id));
    }
    if let Some(environment_id) = environment_id {
        query.and_where(Expr::col(Alias::new("environment_id")).eq(environment_id));
    }
    if let Some(since) = since {
        query.and_where(Expr::col(Alias::new("last_seen_at")).gte(since));
    }
    let count = database
        .query_one(&query.to_owned())
        .await?
        .and_then(|row| row.try_get::<i64>("", "count").ok())
        .unwrap_or(0);
    u64::try_from(count).map_err(|_| DbErr::Custom("negative measured device count".into()))
}

fn percentage(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        return 0.0;
    }
    ((numerator as f64 / denominator as f64) * 1000.0).round() / 10.0
}
