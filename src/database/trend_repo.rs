use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, Utc};
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr,
    sea_query::{Alias, Expr, ExprTrait, Func, Order, Query},
};

use super::rollup_repo;

const GLOBAL_ENVIRONMENT: &str = "*";

#[derive(Clone, Debug)]
pub struct TrendPoint {
    pub day: String,
    pub events: u64,
    pub users: u64,
}

/// Returns a rollup-backed trend only when the requested window has daily semantics.
///
/// `1d` remains raw/hourly. `365d` and all-time remain raw/monthly because exact monthly
/// distinct users cannot be derived by summing per-day distinct counts.
pub async fn application_daily_hybrid(
    database: &DatabaseConnection,
    application_id: &str,
    environment_id: Option<&str>,
    days: Option<u32>,
    since_ts: Option<i64>,
) -> Result<Option<Vec<TrendPoint>>, DbErr> {
    if !supports_daily_rollup(days) {
        return Ok(None);
    }

    let since_day = since_ts
        .and_then(day_for_timestamp)
        .unwrap_or_else(|| "0001-01-01".to_owned());
    let Some(points) = rollup_repo::application_event_trend_hybrid(
        database,
        application_id,
        environment_id,
        &since_day,
    )
    .await?
    else {
        return Ok(None);
    };

    let mut points = points
        .into_iter()
        .map(|point| {
            (
                point.day.clone(),
                TrendPoint {
                    day: point.day,
                    events: point.events,
                    users: point.users,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();

    if let Some(since) = since_ts {
        replace_partial_start_day(
            database,
            &mut points,
            Some(application_id),
            environment_id,
            since,
        )
        .await?;
    }

    Ok(Some(points.into_values().collect()))
}

pub async fn global_daily_hybrid(
    database: &DatabaseConnection,
    days: Option<u32>,
    since_ts: Option<i64>,
) -> Result<Option<Vec<TrendPoint>>, DbErr> {
    if !supports_daily_rollup(days) || !rollup_repo::rollup_backfill_seeded(database).await? {
        return Ok(None);
    }

    let since_day = since_ts
        .and_then(day_for_timestamp)
        .unwrap_or_else(|| "0001-01-01".to_owned());

    let rollups = Query::select()
        .column(Alias::new("day"))
        .expr_as(Func::sum(Expr::col(Alias::new("events"))), Alias::new("events"))
        .expr_as(Func::sum(Expr::col(Alias::new("users"))), Alias::new("users"))
        .from(Alias::new("telemetry_daily_rollups"))
        .and_where(Expr::col(Alias::new("environment_id")).eq(GLOBAL_ENVIRONMENT))
        .and_where(Expr::col(Alias::new("day")).gte(&since_day))
        .group_by_col(Alias::new("day"))
        .order_by(Alias::new("day"), Order::Asc)
        .to_owned();

    let mut points = BTreeMap::<String, TrendPoint>::new();
    for row in database.query_all(&rollups).await? {
        let day: String = row.try_get("", "day")?;
        points.insert(
            day.clone(),
            TrendPoint {
                day,
                events: positive_u64(row.try_get::<i64>("", "events").unwrap_or(0)),
                users: positive_u64(row.try_get::<i64>("", "users").unwrap_or(0)),
            },
        );
    }

    // If any app is dirty for a day, replace the whole global day from raw events. This avoids
    // mixing stale rollup values for one app with fresh values for another.
    let dirty = Query::select()
        .column(Alias::new("day"))
        .from(Alias::new("telemetry_dirty_days"))
        .and_where(Expr::col(Alias::new("environment_id")).eq(GLOBAL_ENVIRONMENT))
        .and_where(Expr::col(Alias::new("day")).gte(&since_day))
        .distinct()
        .to_owned();
    let dirty_days = database
        .query_all(&dirty)
        .await?
        .into_iter()
        .filter_map(|row| row.try_get::<String>("", "day").ok())
        .collect::<Vec<_>>();

    if !dirty_days.is_empty() {
        let raw = Query::select()
            .column(Alias::new("day"))
            .expr_as(Func::count(Expr::col(Alias::new("id"))), Alias::new("events"))
            .expr_as(
                Expr::cust("COUNT(DISTINCT anonymous_id)"),
                Alias::new("users"),
            )
            .from(Alias::new("events"))
            .and_where(Expr::col(Alias::new("day")).is_in(dirty_days.iter().map(String::as_str)))
            .group_by_col(Alias::new("day"))
            .to_owned();

        let mut seen = std::collections::HashSet::new();
        for row in database.query_all(&raw).await? {
            let day: String = row.try_get("", "day")?;
            seen.insert(day.clone());
            points.insert(
                day.clone(),
                TrendPoint {
                    day,
                    events: positive_u64(row.try_get::<i64>("", "events").unwrap_or(0)),
                    users: positive_u64(row.try_get::<i64>("", "users").unwrap_or(0)),
                },
            );
        }
        for day in dirty_days {
            if !seen.contains(&day) {
                points.remove(&day);
            }
        }
    }

    if let Some(since) = since_ts {
        replace_partial_start_day(database, &mut points, None, None, since).await?;
    }

    Ok(Some(points.into_values().collect()))
}

fn supports_daily_rollup(days: Option<u32>) -> bool {
    matches!(days, Some(days) if days > 1 && days != 365)
}

async fn replace_partial_start_day(
    database: &DatabaseConnection,
    points: &mut BTreeMap<String, TrendPoint>,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since_ts: i64,
) -> Result<(), DbErr> {
    let Some(day) = day_for_timestamp(since_ts) else {
        return Ok(());
    };
    let Some(next_day_ts) = next_day_timestamp(&day) else {
        return Ok(());
    };

    let mut query = Query::select();
    query
        .expr_as(Func::count(Expr::col(Alias::new("id"))), Alias::new("events"))
        .expr_as(
            Expr::cust("COUNT(DISTINCT anonymous_id)"),
            Alias::new("users"),
        )
        .from(Alias::new("events"))
        .and_where(Expr::col(Alias::new("timestamp")).gte(since_ts))
        .and_where(Expr::col(Alias::new("timestamp")).lt(next_day_ts));
    if let Some(application_id) = application_id {
        query.and_where(Expr::col(Alias::new("application_id")).eq(application_id));
    }
    if let Some(environment_id) = environment_id {
        query.and_where(Expr::col(Alias::new("environment_id")).eq(environment_id));
    }

    let row = database.query_one(&query).await?;
    let events = row
        .as_ref()
        .and_then(|row| row.try_get::<i64>("", "events").ok())
        .map(positive_u64)
        .unwrap_or(0);
    let users = row
        .as_ref()
        .and_then(|row| row.try_get::<i64>("", "users").ok())
        .map(positive_u64)
        .unwrap_or(0);

    if events == 0 {
        points.remove(&day);
    } else {
        points.insert(day.clone(), TrendPoint { day, events, users });
    }
    Ok(())
}

fn day_for_timestamp(timestamp: i64) -> Option<String> {
    DateTime::<Utc>::from_timestamp_millis(timestamp).map(|value| value.format("%Y-%m-%d").to_string())
}

fn next_day_timestamp(day: &str) -> Option<i64> {
    let date = NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()?;
    let next = date.succ_opt()?;
    Some(next.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis())
}

fn positive_u64(value: i64) -> u64 {
    value.max(0) as u64
}
