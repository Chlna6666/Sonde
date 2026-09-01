use std::collections::BTreeMap;

use sea_orm::{
    ConnectionTrait, DbErr,
    sea_query::{Alias, Expr, ExprTrait, Func, Query},
};

use super::{query::insert_batch_ignore_conflicts, telemetry::TelemetryScope};

pub const SESSION_IDLE_MILLIS: i64 = 30 * 60_000;

#[derive(Clone, Debug, Default)]
pub struct SessionSummary {
    pub total_sessions: u64,
    pub total_active_millis: u64,
    pub average_session_millis: u64,
}

#[derive(Clone, Debug)]
pub struct SessionBucket {
    pub bucket: String,
    pub sessions: u64,
    pub active_millis: u64,
}

#[derive(Clone, Copy)]
enum SessionGranularity {
    Hour,
    Day,
    Month,
}

pub async fn record(
    database: &impl ConnectionTrait,
    scope: &TelemetryScope,
    device_hash: &str,
    session_id: &str,
    previous_seen_at: i64,
    received_at: i64,
    new_session: bool,
) -> Result<(), DbErr> {
    let gap = received_at.saturating_sub(previous_seen_at);
    let active_delta = if !new_session && gap > 0 && gap <= SESSION_IDLE_MILLIS {
        gap
    } else {
        0
    };
    let started_at = if new_session {
        received_at
    } else {
        std::cmp::min(previous_seen_at, received_at)
    };
    let inserted = insert_batch_ignore_conflicts(
        database,
        "telemetry_device_sessions",
        &[
            "id",
            "application_id",
            "environment_id",
            "device_hash",
            "started_at",
            "last_seen_at",
            "active_millis",
            "request_count",
            "updated_at",
        ],
        vec![vec![
            session_id.to_owned().into(),
            scope.application_id.clone().into(),
            scope.environment_id.clone().into(),
            device_hash.to_owned().into(),
            started_at.into(),
            received_at.into(),
            active_delta.into(),
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

    let update = Query::update()
        .table(Alias::new("telemetry_device_sessions"))
        .value(
            Alias::new("last_seen_at"),
            Expr::cust_with_values(
                "CASE WHEN last_seen_at < ? THEN ? ELSE last_seen_at END",
                [received_at, received_at],
            ),
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
        .and_where(Expr::col(Alias::new("id")).eq(session_id))
        .to_owned();
    database.execute(&update).await?;
    Ok(())
}

pub async fn summary(
    database: &impl ConnectionTrait,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since: Option<i64>,
    until: Option<i64>,
) -> Result<SessionSummary, DbErr> {
    let mut query = Query::select();
    query
        .column(Alias::new("active_millis"))
        .from(Alias::new("telemetry_device_sessions"));
    apply_scope(&mut query, application_id, environment_id);
    if let Some(since) = since {
        query.and_where(Expr::col(Alias::new("started_at")).gte(since));
    }
    if let Some(until) = until {
        query.and_where(Expr::col(Alias::new("started_at")).lt(until));
    }

    let rows = database.query_all(&query.to_owned()).await?;
    let total_sessions = u64::try_from(rows.len()).unwrap_or(u64::MAX);
    let mut total_active_millis = 0_u64;
    for row in rows {
        total_active_millis = total_active_millis.saturating_add(nonnegative(
            row.try_get::<i64>("", "active_millis").unwrap_or(0),
        ));
    }
    Ok(SessionSummary {
        total_sessions,
        total_active_millis,
        average_session_millis: if total_sessions == 0 {
            0
        } else {
            total_active_millis / total_sessions
        },
    })
}

pub async fn buckets(
    database: &impl ConnectionTrait,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since: Option<i64>,
    days: Option<u32>,
) -> Result<Vec<SessionBucket>, DbErr> {
    let granularity = match days {
        Some(1) => SessionGranularity::Hour,
        Some(365) | None => SessionGranularity::Month,
        _ => SessionGranularity::Day,
    };
    let mut query = Query::select();
    query
        .columns(["started_at", "active_millis"].map(Alias::new))
        .from(Alias::new("telemetry_device_sessions"));
    apply_scope(&mut query, application_id, environment_id);
    if let Some(since) = since {
        query.and_where(Expr::col(Alias::new("started_at")).gte(since));
    }

    let mut buckets = BTreeMap::<String, (u64, u64)>::new();
    for row in database.query_all(&query.to_owned()).await? {
        let started_at: i64 = row.try_get("", "started_at")?;
        let bucket = session_bucket(started_at, granularity);
        let active_millis = nonnegative(row.try_get::<i64>("", "active_millis").unwrap_or(0));
        buckets
            .entry(bucket)
            .and_modify(|value| {
                value.0 = value.0.saturating_add(1);
                value.1 = value.1.saturating_add(active_millis);
            })
            .or_insert((1, active_millis));
    }
    Ok(buckets
        .into_iter()
        .map(|(bucket, (sessions, active_millis))| SessionBucket {
            bucket,
            sessions,
            active_millis,
        })
        .collect())
}

pub async fn sessions_before(
    database: &impl ConnectionTrait,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    before: i64,
) -> Result<u64, DbErr> {
    let mut query = Query::select();
    query
        .expr_as(Func::count(Expr::col(Alias::new("id"))), Alias::new("count"))
        .from(Alias::new("telemetry_device_sessions"))
        .and_where(Expr::col(Alias::new("started_at")).lt(before));
    apply_scope(&mut query, application_id, environment_id);
    let count = database
        .query_one(&query.to_owned())
        .await?
        .and_then(|row| row.try_get::<i64>("", "count").ok())
        .unwrap_or(0);
    Ok(nonnegative(count))
}

fn session_bucket(timestamp: i64, granularity: SessionGranularity) -> String {
    let Some(value) = chrono::DateTime::from_timestamp_millis(timestamp) else {
        return match granularity {
            SessionGranularity::Hour => "1970-01-01 00:00".into(),
            SessionGranularity::Day => "1970-01-01".into(),
            SessionGranularity::Month => "1970-01".into(),
        };
    };
    match granularity {
        SessionGranularity::Hour => value.format("%Y-%m-%d %H:00").to_string(),
        SessionGranularity::Day => value.format("%Y-%m-%d").to_string(),
        SessionGranularity::Month => value.format("%Y-%m").to_string(),
    }
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

fn nonnegative(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}
