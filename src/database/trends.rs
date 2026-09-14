use std::collections::{BTreeMap, HashSet};

use chrono::{DateTime, NaiveDate, Utc};
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr,
    sea_query::{Alias, Expr, ExprTrait, Func, Order, Query},
};

use super::{rollups, user_rollup};

const GLOBAL_ENVIRONMENT: &str = "*";
const MAX_DIRTY_DAY_BINDS: usize = 400;

#[derive(Clone, Debug)]
pub struct TrendPoint {
    pub day: String,
    pub events: u64,
    pub users: u64,
}

/// Returns a rollup-backed trend for every non-hourly statistics window.
///
/// Normal multi-day windows retain daily buckets. `365d` and all-time project daily event rollups
/// into months and derive exact monthly active users by unioning the compact daily user sets instead
/// of summing daily distinct counts.
pub async fn application_daily_hybrid(
    database: &DatabaseConnection,
    application_id: &str,
    environment_id: Option<&str>,
    days: Option<u32>,
    since_ts: Option<i64>,
) -> Result<Option<Vec<TrendPoint>>, DbErr> {
    if !supports_rollup(days) {
        return Ok(None);
    }

    let since_day = since_ts
        .and_then(day_for_timestamp)
        .unwrap_or_else(|| "0001-01-01".to_owned());
    if application_dirty_backlog_exceeds(database, application_id, environment_id, &since_day)
        .await?
    {
        return Ok(None);
    }
    let Some(points) = rollups::application_event_trend_hybrid(
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

    if monthly_buckets(days) {
        let Some(users) = user_rollup::user_growth_hybrid(
            database,
            Some(application_id),
            environment_id,
            since_ts,
            true,
        )
        .await?
        else {
            return Ok(None);
        };
        return project_monthly(points, users).map(Some);
    }

    Ok(Some(points.into_values().collect()))
}

pub async fn global_daily_hybrid(
    database: &DatabaseConnection,
    days: Option<u32>,
    since_ts: Option<i64>,
) -> Result<Option<Vec<TrendPoint>>, DbErr> {
    if !supports_rollup(days) || !rollups::rollup_backfill_seeded(database).await? {
        return Ok(None);
    }

    let since_day = since_ts
        .and_then(day_for_timestamp)
        .unwrap_or_else(|| "0001-01-01".to_owned());

    // Read one already-aggregated row per application/day and combine in Rust. PostgreSQL promotes
    // SUM(BIGINT) to NUMERIC; avoiding SQL SUM keeps the row type identical on SQLite/MySQL/Postgres.
    let rollups_query = Query::select()
        .columns(["day", "events"].map(Alias::new))
        .from(Alias::new("telemetry_daily_rollups"))
        .and_where(Expr::col(Alias::new("environment_id")).eq(GLOBAL_ENVIRONMENT))
        .and_where(Expr::col(Alias::new("day")).gte(&since_day))
        .order_by(Alias::new("day"), Order::Asc)
        .to_owned();

    let mut points = BTreeMap::<String, TrendPoint>::new();
    for row in database.query_all(&rollups_query).await? {
        let day: String = row.try_get("", "day")?;
        let events = positive_u64(row.try_get::<i64>("", "events").unwrap_or(0));
        points
            .entry(day.clone())
            .and_modify(|point| point.events = point.events.saturating_add(events))
            .or_insert(TrendPoint {
                day,
                events,
                users: 0,
            });
    }

    // If any app has event-dirty data for a day, replace the whole global event count for that day
    // from raw events. Metric/log/error-only markers must not invalidate event trend caches.
    let dirty = Query::select()
        .column(Alias::new("day"))
        .from(Alias::new("telemetry_dirty_days"))
        .and_where(Expr::col(Alias::new("environment_id")).eq(GLOBAL_ENVIRONMENT))
        .and_where(Expr::col(Alias::new("day")).gte(&since_day))
        .and_where(rollups::dirty_source_condition(rollups::DIRTY_SOURCE_EVENT))
        .distinct()
        .limit((MAX_DIRTY_DAY_BINDS + 1) as u64)
        .to_owned();
    let dirty_days = database
        .query_all(&dirty)
        .await?
        .into_iter()
        .filter_map(|row| row.try_get::<String>("", "day").ok())
        .collect::<Vec<_>>();
    if dirty_days.len() > MAX_DIRTY_DAY_BINDS {
        return Ok(None);
    }

    if !dirty_days.is_empty() {
        let raw = Query::select()
            .column(Alias::new("day"))
            .expr_as(
                Func::count(Expr::col(Alias::new("id"))),
                Alias::new("events"),
            )
            .from(Alias::new("events"))
            .and_where(Expr::col(Alias::new("day")).is_in(dirty_days.iter().map(String::as_str)))
            .group_by_col(Alias::new("day"))
            .to_owned();

        let mut seen = HashSet::new();
        for row in database.query_all(&raw).await? {
            let day: String = row.try_get("", "day")?;
            seen.insert(day.clone());
            points.insert(
                day.clone(),
                TrendPoint {
                    day,
                    events: positive_u64(row.try_get::<i64>("", "events").unwrap_or(0)),
                    users: 0,
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
        replace_partial_start_events(database, &mut points, since).await?;
    }

    let monthly = monthly_buckets(days);
    let Some(users) =
        user_rollup::user_growth_hybrid(database, None, None, since_ts, monthly).await?
    else {
        return Ok(None);
    };

    if monthly {
        return project_monthly(points, users).map(Some);
    }

    apply_user_counts(&mut points, users);
    Ok(Some(points.into_values().collect()))
}

fn supports_rollup(days: Option<u32>) -> bool {
    days != Some(1)
}

fn monthly_buckets(days: Option<u32>) -> bool {
    matches!(days, Some(365) | None)
}

async fn application_dirty_backlog_exceeds(
    database: &DatabaseConnection,
    application_id: &str,
    environment_id: Option<&str>,
    since_day: &str,
) -> Result<bool, DbErr> {
    let rollup_environment = environment_id.unwrap_or(GLOBAL_ENVIRONMENT);
    let query = Query::select()
        .column(Alias::new("id"))
        .from(Alias::new("telemetry_dirty_days"))
        .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
        .and_where(Expr::col(Alias::new("environment_id")).eq(rollup_environment))
        .and_where(Expr::col(Alias::new("day")).gte(since_day))
        .and_where(rollups::dirty_source_condition(rollups::DIRTY_SOURCE_EVENT))
        .limit((MAX_DIRTY_DAY_BINDS + 1) as u64)
        .to_owned();
    Ok(database.query_all(&query).await?.len() > MAX_DIRTY_DAY_BINDS)
}

fn apply_user_counts(
    points: &mut BTreeMap<String, TrendPoint>,
    users: Vec<user_rollup::UserGrowthBucket>,
) {
    for user_bucket in users {
        points
            .entry(user_bucket.bucket.clone())
            .and_modify(|point| point.users = user_bucket.active_users)
            .or_insert(TrendPoint {
                day: user_bucket.bucket,
                events: 0,
                users: user_bucket.active_users,
            });
    }
}

fn project_monthly(
    daily: BTreeMap<String, TrendPoint>,
    users: Vec<user_rollup::UserGrowthBucket>,
) -> Result<Vec<TrendPoint>, DbErr> {
    let mut monthly = BTreeMap::<String, TrendPoint>::new();
    for point in daily.into_values() {
        let month = point
            .day
            .get(..7)
            .ok_or_else(|| DbErr::Custom("invalid daily trend day".into()))?
            .to_owned();
        monthly
            .entry(month.clone())
            .and_modify(|value| value.events = value.events.saturating_add(point.events))
            .or_insert(TrendPoint {
                day: month,
                events: point.events,
                users: 0,
            });
    }
    apply_user_counts(&mut monthly, users);
    Ok(monthly.into_values().collect())
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
        .expr_as(
            Func::count(Expr::col(Alias::new("id"))),
            Alias::new("events"),
        )
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

async fn replace_partial_start_events(
    database: &DatabaseConnection,
    points: &mut BTreeMap<String, TrendPoint>,
    since_ts: i64,
) -> Result<(), DbErr> {
    let Some(day) = day_for_timestamp(since_ts) else {
        return Ok(());
    };
    let Some(next_day_ts) = next_day_timestamp(&day) else {
        return Ok(());
    };
    let query = Query::select()
        .expr_as(
            Func::count(Expr::col(Alias::new("id"))),
            Alias::new("events"),
        )
        .from(Alias::new("events"))
        .and_where(Expr::col(Alias::new("timestamp")).gte(since_ts))
        .and_where(Expr::col(Alias::new("timestamp")).lt(next_day_ts))
        .to_owned();
    let events = database
        .query_one(&query)
        .await?
        .and_then(|row| row.try_get::<i64>("", "events").ok())
        .map(positive_u64)
        .unwrap_or(0);
    if events == 0 {
        points.remove(&day);
    } else {
        points.insert(
            day.clone(),
            TrendPoint {
                day,
                events,
                users: 0,
            },
        );
    }
    Ok(())
}

fn day_for_timestamp(timestamp: i64) -> Option<String> {
    DateTime::<Utc>::from_timestamp_millis(timestamp)
        .map(|value| value.format("%Y-%m-%d").to_string())
}

fn next_day_timestamp(day: &str) -> Option<i64> {
    let date = NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()?;
    let next = date.succ_opt()?;
    Some(next.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis())
}

fn positive_u64(value: i64) -> u64 {
    std::cmp::max(value, 0) as u64
}
