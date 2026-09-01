use std::collections::BTreeMap;

use sea_orm::{DatabaseConnection, DbErr};

use super::{device_activity, device_session};

#[derive(Clone, Debug, Default)]
pub struct ActivitySummary {
    pub active_millis: u64,
    pub lifetime_active_millis: u64,
    pub sessions: u64,
    pub lifetime_sessions: u64,
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
                cumulative_active_millis: point.cumulative_active_millis,
                cumulative_sessions: 0,
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
            });
    }

    let mut cumulative_active_millis = 0_u64;
    let mut cumulative_sessions = 0_u64;
    for point in buckets.values_mut() {
        cumulative_active_millis = cumulative_active_millis.saturating_add(point.active_millis);
        cumulative_sessions = cumulative_sessions.saturating_add(point.sessions);
        point.cumulative_active_millis = cumulative_active_millis;
        point.cumulative_sessions = cumulative_sessions;
    }

    Ok(ActivityStats {
        summary: ActivitySummary {
            active_millis,
            lifetime_active_millis,
            sessions: sessions.total_sessions,
            lifetime_sessions: lifetime_sessions.total_sessions,
            average_session_millis: sessions.average_session_millis,
            average_active_millis_per_device: if active_devices == 0 {
                0
            } else {
                active_millis / active_devices
            },
            stickiness_pct: if mau == 0 {
                0.0
            } else {
                ((dau as f64 / mau as f64) * 1000.0).round() / 10.0
            },
        },
        trend: buckets.into_values().collect(),
    })
}
