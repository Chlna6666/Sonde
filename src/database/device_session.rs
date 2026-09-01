use std::collections::BTreeMap;

use sea_orm::{
    ConnectionTrait, DbBackend, DbErr,
    sea_query::{Alias, Expr, ExprTrait, Func, Order, Query},
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
        .expr_as(Func::count(Expr::col(Alias::new("id"))), Alias::new("sessions"))
        .expr_as(
            Func::sum(Expr::col(Alias::new("active_millis"))),
            Alias::new("active_millis"),
        )
        .from(Alias::new("telemetry_device_sessions"));
    apply_scope(&mut query, application_id, environment_id);
    if let Some(since) = since {
        query.and_where(Expr::col(Alias::new("started_at")).gte(since));
    }
    if let Some(until) = until {
        query.and_where(Expr::col(Alias::new("started_at")).lt(until));
    }
    let Some(row) = database.query_one(&query.to_owned()).await? else {
        return Ok(SessionSummary::default());
    };
    let total_sessions = nonnegative(row.try_get::<i64>("", "sessions").unwrap_or(0));
    let total_active_millis = nonnegative(row.try_get::<i64>("", "active_millis").unwrap_or(0));
    let average_session_millis = if total_sessions == 0 {
        0
    } else {
        total_active_millis / total_sessions
    };
    Ok(SessionSummary {
        total_sessions,
        total_active_millis,
        average_session_millis,
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
    let bucket_expr = started_bucket_expr(database.get_database_backend(), granularity)?;
    let mut query = Query::select();
    query
        .expr_as(Expr::cust(bucket_expr), Alias::new("bucket"))
        .expr_as(Func::count(Expr::col(Alias::new("id"))), Alias::new("sessions"))
        .expr_as(
            Func::sum(Expr::col(Alias::new("active_millis"))),
            Alias::new("active_millis"),
        )
        .from(Alias::new("telemetry_device_sessions"))
        .group_by_col(Alias::new("bucket"))
        .order_by(Alias::new("bucket"), Order::Asc);
    apply_scope(&mut query, application_id, environment_id);
    if let Some(since) = since {
        query.and_where(Expr::col(Alias::new("started_at")).gte(since));
    }

    database
        .query_all(&query.to_owned())
        .await?
        .into_iter()
        .map(|row| {
            Ok(SessionBucket {
                bucket: row.try_get("", "bucket")?,
                sessions: nonnegative(row.try_get::<i64>("", "sessions").unwrap_or(0)),
                active_millis: nonnegative(
                    row.try_get::<i64>("", "active_millis").unwrap_or(0),
                ),
            })
        })
        .collect()
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

fn started_bucket_expr(
    backend: DbBackend,
    granularity: SessionGranularity,
) -> Result<String, DbErr> {
    let expression = match (backend, granularity) {
        (DbBackend::Postgres, SessionGranularity::Hour) => {
            "to_char(to_timestamp(started_at / 1000.0), 'YYYY-MM-DD HH24:00')"
        }
        (DbBackend::Postgres, SessionGranularity::Day) => {
            "to_char(to_timestamp(started_at / 1000.0), 'YYYY-MM-DD')"
        }
        (DbBackend::Postgres, SessionGranularity::Month) => {
            "to_char(to_timestamp(started_at / 1000.0), 'YYYY-MM')"
        }
        (DbBackend::MySql, SessionGranularity::Hour) => {
            "DATE_FORMAT(FROM_UNIXTIME(started_at / 1000), '%Y-%m-%d %H:00')"
        }
        (DbBackend::MySql, SessionGranularity::Day) => {
            "DATE_FORMAT(FROM_UNIXTIME(started_at / 1000), '%Y-%m-%d')"
        }
        (DbBackend::MySql, SessionGranularity::Month) => {
            "DATE_FORMAT(FROM_UNIXTIME(started_at / 1000), '%Y-%m')"
        }
        (DbBackend::Sqlite, SessionGranularity::Hour) => {
            "strftime('%Y-%m-%d %H:00', started_at / 1000, 'unixepoch')"
        }
        (DbBackend::Sqlite, SessionGranularity::Day) => {
            "strftime('%Y-%m-%d', started_at / 1000, 'unixepoch')"
        }
        (DbBackend::Sqlite, SessionGranularity::Month) => {
            "strftime('%Y-%m', started_at / 1000, 'unixepoch')"
        }
        _ => return Err(DbErr::Custom("unsupported database backend for device sessions".into())),
    };
    Ok(expression.to_owned())
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
