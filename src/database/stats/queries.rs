use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, DbErr,
    sea_query::{Alias, Expr, ExprTrait, Func, Query},
};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

fn time_bucket_expr(backend: DbBackend, days: Option<u32>) -> String {
    match backend {
        DbBackend::Postgres => match days {
            Some(1) => "to_char(to_timestamp(timestamp / 1000.0), 'YYYY-MM-DD HH24:00')".into(),
            Some(365) => "to_char(to_timestamp(timestamp / 1000.0), 'YYYY-MM')".into(),
            None => "to_char(to_timestamp(timestamp / 1000.0), 'YYYY-MM')".into(),
            _ => "day".into(),
        },
        DbBackend::MySql => match days {
            Some(1) => "DATE_FORMAT(FROM_UNIXTIME(timestamp / 1000), '%Y-%m-%d %H:00')".into(),
            Some(365) => "DATE_FORMAT(FROM_UNIXTIME(timestamp / 1000), '%Y-%m')".into(),
            None => "DATE_FORMAT(FROM_UNIXTIME(timestamp / 1000), '%Y-%m')".into(),
            _ => "day".into(),
        },
        DbBackend::Sqlite => match days {
            Some(1) => "strftime('%Y-%m-%d %H:00', timestamp / 1000, 'unixepoch')".into(),
            Some(365) => "strftime('%Y-%m', timestamp / 1000, 'unixepoch')".into(),
            None => "strftime('%Y-%m', timestamp / 1000, 'unixepoch')".into(),
            _ => "day".into(),
        },
        _ => "day".into(),
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrowthMetrics {
    pub events_growth_pct: Option<f64>,
    pub users_growth_pct: Option<f64>,
    pub new_users: u64,
    pub returning_users: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserGrowthPoint {
    pub bucket: String,
    pub new_users: u64,
    pub cumulative_users: u64,
    pub active_users: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionShare {
    pub version: String,
    pub count: u64,
    pub percentage: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionTimelinePoint {
    pub bucket: String,
    pub total_events: u64,
    pub versions: Vec<VersionShare>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionSeriesPoint {
    pub day: String,
    pub count: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionSeries {
    pub version: String,
    pub total_count: u64,
    pub data: Vec<VersionSeriesPoint>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatsOverview {
    pub total_events: u64,
    pub active_users: u64,
    pub total_errors: u64,
    pub avg_daily_events: u64,
    pub total_users: u64,
    pub dau: u64,
    pub wau: u64,
    pub mau: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyTrendPoint {
    pub day: String,
    pub events: u64,
    pub users: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DistributionItem {
    pub name: String,
    pub count: u64,
    pub percentage: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub applications: u64,
    pub events_24h: u64,
    pub metrics_24h: u64,
    pub logs_24h: u64,
    pub errors_24h: u64,
    pub active_users_24h: u64,
    pub total_users: u64,
    pub dau: u64,
    pub wau: u64,
    pub mau: u64,
    pub growth: GrowthMetrics,
    pub trend: Vec<DailyTrendPoint>,
    pub user_growth: Vec<UserGrowthPoint>,
    pub version_timeline: Vec<VersionTimelinePoint>,
    pub version_series: Vec<VersionSeries>,
    pub app_versions: Vec<DistributionItem>,
    pub os_families: Vec<DistributionItem>,
    pub operating_systems: Vec<DistributionItem>,
    pub build_distribution: Vec<DistributionItem>,
    pub system_languages: Vec<DistributionItem>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppTelemetryStats {
    pub overview: AppStatsOverview,
    pub growth: GrowthMetrics,
    pub trend: Vec<DailyTrendPoint>,
    pub user_growth: Vec<UserGrowthPoint>,
    pub version_timeline: Vec<VersionTimelinePoint>,
    pub version_series: Vec<VersionSeries>,
    pub app_versions: Vec<DistributionItem>,
    pub os_families: Vec<DistributionItem>,
    pub operating_systems: Vec<DistributionItem>,
    pub build_distribution: Vec<DistributionItem>,
    pub system_languages: Vec<DistributionItem>,
}

pub async fn overview(
    database: &DatabaseConnection,
    application_id: Option<&str>,
    days: Option<u32>,
) -> Result<Overview, DbErr> {
    let now = chrono::Utc::now().timestamp_millis();
    let since_24h = now - 86_400_000;
    let since_7d = now - 7 * 86_400_000;
    let since_30d = now - 30 * 86_400_000;

    let applications = match application_id {
        Some(app_id) => {
            if crate::database::applications::get_application(database, app_id)
                .await?
                .is_some()
            {
                1
            } else {
                0
            }
        }
        None => count(database, "applications", None).await?,
    };
    let events_24h = telemetry_count(
        database,
        crate::database::telemetry_count::RollupCountKind::Events,
        application_id,
        None,
        Some(since_24h),
        None,
    )
    .await?;
    let metrics_24h = telemetry_count(
        database,
        crate::database::telemetry_count::RollupCountKind::Metrics,
        application_id,
        None,
        Some(since_24h),
        None,
    )
    .await?;
    let logs_24h = telemetry_count(
        database,
        crate::database::telemetry_count::RollupCountKind::Logs,
        application_id,
        None,
        Some(since_24h),
        None,
    )
    .await?;
    let errors_24h = telemetry_count(
        database,
        crate::database::telemetry_count::RollupCountKind::ErrorLogs,
        application_id,
        None,
        Some(since_24h),
        None,
    )
    .await?;

    let active_users_24h = distinct_users(database, since_24h, application_id, None).await?;
    let total_users = count_distinct_users(database, application_id, None, None).await?;
    let dau = active_users_24h;
    let wau = distinct_users(database, since_7d, application_id, None).await?;
    let mau = distinct_users(database, since_30d, application_id, None).await?;

    let bucket_expr = time_bucket_expr(database.get_database_backend(), days);
    let (since_ts, prev_since_ts, prev_until_ts) = statistics_window(now, days);
    let growth = compute_growth(
        database,
        application_id,
        None,
        since_ts,
        prev_since_ts,
        prev_until_ts,
    )
    .await?;
    let user_growth = compute_user_growth(database, application_id, None, since_ts, days).await?;
    let event_trend =
        event_trend(database, application_id, None, days, since_ts, &bucket_expr).await?;
    let trend = merge_activity_trend(event_trend, &user_growth);

    let version_timeline =
        compute_version_timeline(database, application_id, None, since_ts, &bucket_expr).await?;
    let version_series =
        compute_version_series(database, application_id, None, since_ts, &bucket_expr).await?;

    let app_versions = app_version_distribution(database, application_id, None, since_ts).await?;
    let os_families = os_family_distribution(database, application_id, None, since_ts).await?;
    let operating_systems =
        operating_systems_distribution(database, application_id, None, since_ts).await?;
    let build_distribution = build_distribution(database, application_id, None, since_ts).await?;
    let system_languages =
        system_language_distribution(database, application_id, None, since_ts).await?;

    Ok(Overview {
        applications,
        events_24h,
        metrics_24h,
        logs_24h,
        errors_24h,
        active_users_24h,
        total_users,
        dau,
        wau,
        mau,
        growth,
        trend,
        user_growth,
        version_timeline,
        version_series,
        app_versions,
        os_families,
        operating_systems,
        build_distribution,
        system_languages,
    })
}

pub async fn application_stats(
    database: &DatabaseConnection,
    application_id: &str,
    environment_id: Option<&str>,
    days: Option<u32>,
) -> Result<AppTelemetryStats, DbErr> {
    let now = chrono::Utc::now().timestamp_millis();
    let since_24h = now - 86_400_000;
    let since_7d = now - 7 * 86_400_000;
    let since_30d = now - 30 * 86_400_000;
    let bucket_expr = time_bucket_expr(database.get_database_backend(), days);
    let (since_ts, prev_since_ts, prev_until_ts) = statistics_window(now, days);

    let total_events = telemetry_count(
        database,
        crate::database::telemetry_count::RollupCountKind::Events,
        Some(application_id),
        environment_id,
        since_ts,
        None,
    )
    .await?;
    let active_users =
        count_distinct_users(database, Some(application_id), environment_id, since_ts).await?;
    let total_users =
        count_distinct_users(database, Some(application_id), environment_id, None).await?;
    let dau = distinct_users(database, since_24h, Some(application_id), environment_id).await?;
    let wau = distinct_users(database, since_7d, Some(application_id), environment_id).await?;
    let mau = distinct_users(database, since_30d, Some(application_id), environment_id).await?;
    let total_errors = telemetry_count(
        database,
        crate::database::telemetry_count::RollupCountKind::ErrorLogs,
        Some(application_id),
        environment_id,
        since_ts,
        None,
    )
    .await?;

    let growth = compute_growth(
        database,
        Some(application_id),
        environment_id,
        since_ts,
        prev_since_ts,
        prev_until_ts,
    )
    .await?;
    let user_growth = compute_user_growth(
        database,
        Some(application_id),
        environment_id,
        since_ts,
        days,
    )
    .await?;
    let event_trend = event_trend(
        database,
        Some(application_id),
        environment_id,
        days,
        since_ts,
        &bucket_expr,
    )
    .await?;
    let trend = merge_activity_trend(event_trend, &user_growth);
    let bucket_count = std::cmp::max(trend.len(), 1) as u64;
    let avg_daily_events = total_events / bucket_count;

    let version_timeline = compute_version_timeline(
        database,
        Some(application_id),
        environment_id,
        since_ts,
        &bucket_expr,
    )
    .await?;
    let version_series = compute_version_series(
        database,
        Some(application_id),
        environment_id,
        since_ts,
        &bucket_expr,
    )
    .await?;

    let app_versions =
        app_version_distribution(database, Some(application_id), environment_id, since_ts).await?;
    let os_families =
        os_family_distribution(database, Some(application_id), environment_id, since_ts).await?;
    let operating_systems =
        operating_systems_distribution(database, Some(application_id), environment_id, since_ts)
            .await?;
    let build_distribution =
        build_distribution(database, Some(application_id), environment_id, since_ts).await?;
    let system_languages =
        system_language_distribution(database, Some(application_id), environment_id, since_ts)
            .await?;

    Ok(AppTelemetryStats {
        overview: AppStatsOverview {
            total_events,
            active_users,
            total_errors,
            avg_daily_events,
            total_users,
            dau,
            wau,
            mau,
        },
        growth,
        trend,
        user_growth,
        version_timeline,
        version_series,
        app_versions,
        os_families,
        operating_systems,
        build_distribution,
        system_languages,
    })
}

fn statistics_window(now: i64, days: Option<u32>) -> (Option<i64>, Option<i64>, Option<i64>) {
    match days {
        Some(1) => (
            Some(now - 86_400_000),
            Some(now - 2 * 86_400_000),
            Some(now - 86_400_000),
        ),
        Some(7) => (
            Some(now - 7 * 86_400_000),
            Some(now - 14 * 86_400_000),
            Some(now - 7 * 86_400_000),
        ),
        Some(30) => (
            Some(now - 30 * 86_400_000),
            Some(now - 60 * 86_400_000),
            Some(now - 30 * 86_400_000),
        ),
        Some(365) => (
            Some(now - 365 * 86_400_000),
            Some(now - 2 * 365 * 86_400_000),
            Some(now - 365 * 86_400_000),
        ),
        Some(days) => {
            let span = days as i64 * 86_400_000;
            (Some(now - span), Some(now - 2 * span), Some(now - span))
        }
        None => (None, None, None),
    }
}

async fn telemetry_count(
    database: &DatabaseConnection,
    kind: crate::database::telemetry_count::RollupCountKind,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since: Option<i64>,
    until: Option<i64>,
) -> Result<u64, DbErr> {
    crate::database::telemetry_count::count_hybrid(
        database,
        kind,
        application_id,
        environment_id,
        since,
        until,
    )
    .await
}

async fn count(
    database: &DatabaseConnection,
    table: &str,
    since: Option<(&str, i64)>,
) -> Result<u64, DbErr> {
    let mut query = Query::select();
    query
        .expr_as(
            Func::count(Expr::col(Alias::new("id"))),
            Alias::new("total"),
        )
        .from(Alias::new(table));
    if let Some((column, value)) = since {
        query.and_where(Expr::col(Alias::new(column)).gte(value));
    }
    let value = database
        .query_one(&query)
        .await?
        .and_then(|row| row.try_get::<i64>("", "total").ok())
        .unwrap_or(0);
    u64::try_from(value).map_err(|_| DbErr::Custom("negative statistics count".into()))
}

async fn distinct_users(
    database: &DatabaseConnection,
    since: i64,
    application_id: Option<&str>,
    environment_id: Option<&str>,
) -> Result<u64, DbErr> {
    crate::database::device_activity::unique_devices(
        database,
        application_id,
        environment_id,
        Some(since),
        None,
    )
    .await
}

async fn count_distinct_users(
    database: &DatabaseConnection,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since: Option<i64>,
) -> Result<u64, DbErr> {
    match since {
        Some(since) => {
            crate::database::device_activity::unique_devices(
                database,
                application_id,
                environment_id,
                Some(since),
                None,
            )
            .await
        }
        None => {
            crate::database::device_activity::total_devices(
                database,
                application_id,
                environment_id,
            )
            .await
        }
    }
}

async fn compute_growth(
    database: &DatabaseConnection,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since_ts: Option<i64>,
    prev_since_ts: Option<i64>,
    prev_until_ts: Option<i64>,
) -> Result<GrowthMetrics, DbErr> {
    if let (Some(since), Some(prev_since), Some(prev_until)) =
        (since_ts, prev_since_ts, prev_until_ts)
    {
        let curr_events = telemetry_count(
            database,
            crate::database::telemetry_count::RollupCountKind::Events,
            application_id,
            environment_id,
            Some(since),
            None,
        )
        .await?;
        let prev_events = telemetry_count(
            database,
            crate::database::telemetry_count::RollupCountKind::Events,
            application_id,
            environment_id,
            Some(prev_since),
            Some(prev_until),
        )
        .await?;
        let curr_users = crate::database::device_activity::unique_devices(
            database,
            application_id,
            environment_id,
            Some(since),
            None,
        )
        .await?;
        let prev_users = crate::database::device_activity::unique_devices(
            database,
            application_id,
            environment_id,
            Some(prev_since),
            Some(prev_until),
        )
        .await?;

        let events_growth_pct = growth_percentage(curr_events, prev_events);
        let users_growth_pct = growth_percentage(curr_users, prev_users);
        let new_users = crate::database::device_activity::new_devices(
            database,
            application_id,
            environment_id,
            since,
            None,
        )
        .await?;
        let returning_users = curr_users.saturating_sub(new_users);
        Ok(GrowthMetrics {
            events_growth_pct,
            users_growth_pct,
            new_users,
            returning_users,
        })
    } else {
        let total_users = crate::database::device_activity::total_devices(
            database,
            application_id,
            environment_id,
        )
        .await?;
        Ok(GrowthMetrics {
            events_growth_pct: None,
            users_growth_pct: None,
            new_users: total_users,
            returning_users: 0,
        })
    }
}

fn growth_percentage(current: u64, previous: u64) -> Option<f64> {
    if previous == 0 {
        return None;
    }
    let percentage = ((current as f64 - previous as f64) / previous as f64) * 100.0;
    Some((percentage * 10.0).round() / 10.0)
}

async fn compute_user_growth(
    database: &DatabaseConnection,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since_ts: Option<i64>,
    days: Option<u32>,
) -> Result<Vec<UserGrowthPoint>, DbErr> {
    Ok(crate::database::device_activity::growth_timeline(
        database,
        application_id,
        environment_id,
        since_ts,
        days,
    )
    .await?
    .into_iter()
    .map(|point| UserGrowthPoint {
        bucket: point.bucket,
        new_users: point.new_devices,
        cumulative_users: point.cumulative_devices,
        active_users: point.active_devices,
    })
    .collect())
}

async fn event_trend(
    database: &DatabaseConnection,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    days: Option<u32>,
    since_ts: Option<i64>,
    bucket_expr: &str,
) -> Result<Vec<DailyTrendPoint>, DbErr> {
    let cached = match application_id {
        Some(application_id) => {
            crate::database::trends::application_daily_hybrid(
                database,
                application_id,
                environment_id,
                days,
                since_ts,
            )
            .await?
        }
        None => crate::database::trends::global_daily_hybrid(database, days, since_ts).await?,
    };
    if let Some(points) = cached {
        return Ok(points
            .into_iter()
            .map(|point| DailyTrendPoint {
                day: point.day,
                events: point.events,
                users: 0,
            })
            .collect());
    }

    let mut query = Query::select();
    query
        .expr_as(
            Expr::cust(bucket_expr.to_owned()),
            Alias::new("bucket_time"),
        )
        .expr_as(
            Func::count(Expr::col(Alias::new("id"))),
            Alias::new("events"),
        )
        .from(Alias::new("events"));
    if let Some(application_id) = application_id {
        query.and_where(Expr::col(Alias::new("application_id")).eq(application_id));
    }
    if let Some(environment_id) = environment_id {
        query.and_where(Expr::col(Alias::new("environment_id")).eq(environment_id));
    }
    if let Some(since) = since_ts {
        query.and_where(Expr::col(Alias::new("timestamp")).gte(since));
    }
    query
        .group_by_col(Alias::new("bucket_time"))
        .order_by(Alias::new("bucket_time"), sea_orm::sea_query::Order::Asc);

    database
        .query_all(&query)
        .await?
        .into_iter()
        .map(|row| {
            Ok(DailyTrendPoint {
                day: row.try_get("", "bucket_time")?,
                events: u64::try_from(row.try_get::<i64>("", "events").unwrap_or(0)).unwrap_or(0),
                users: 0,
            })
        })
        .collect()
}

fn merge_activity_trend(
    event_points: Vec<DailyTrendPoint>,
    activity_points: &[UserGrowthPoint],
) -> Vec<DailyTrendPoint> {
    let mut points = event_points
        .into_iter()
        .map(|point| (point.day.clone(), point))
        .collect::<BTreeMap<_, _>>();
    for activity in activity_points {
        points
            .entry(activity.bucket.clone())
            .and_modify(|point| point.users = activity.active_users)
            .or_insert(DailyTrendPoint {
                day: activity.bucket.clone(),
                events: 0,
                users: activity.active_users,
            });
    }
    points.into_values().collect()
}

async fn compute_version_timeline(
    database: &DatabaseConnection,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since_ts: Option<i64>,
    bucket_expr: &str,
) -> Result<Vec<VersionTimelinePoint>, DbErr> {
    if crate::database::version_dimension::supports_daily_projection(bucket_expr)
        && let Some(points) = crate::database::dimension_rollup::event_dimension_timeline_hybrid(
            database,
            application_id,
            environment_id,
            since_ts,
            crate::database::dimension_rollup::DIMENSION_APP_VERSION,
        )
        .await?
    {
        return Ok(
            crate::database::version_dimension::timeline(&points, bucket_expr)
                .into_iter()
                .map(|bucket| {
                    let versions = bucket
                        .versions
                        .into_iter()
                        .map(|(version, count)| VersionShare {
                            version,
                            count,
                            percentage: percentage(count, bucket.total),
                        })
                        .collect();
                    VersionTimelinePoint {
                        bucket: bucket.bucket,
                        total_events: bucket.total,
                        versions,
                    }
                })
                .collect(),
        );
    }

    let mut query = Query::select();
    query
        .expr_as(
            Expr::cust(bucket_expr.to_owned()),
            Alias::new("bucket_time"),
        )
        .expr_as(
            Expr::cust("COALESCE(app_version, 'unknown')"),
            Alias::new("ver_name"),
        )
        .expr_as(
            Func::count(Expr::col(Alias::new("id"))),
            Alias::new("ver_count"),
        )
        .from(Alias::new("events"));
    if let Some(application_id) = application_id {
        query.and_where(Expr::col(Alias::new("application_id")).eq(application_id));
    }
    if let Some(environment_id) = environment_id {
        query.and_where(Expr::col(Alias::new("environment_id")).eq(environment_id));
    }
    if let Some(since) = since_ts {
        query.and_where(Expr::col(Alias::new("timestamp")).gte(since));
    }
    query
        .group_by_col(Alias::new("bucket_time"))
        .group_by_col(Alias::new("ver_name"))
        .order_by(Alias::new("bucket_time"), sea_orm::sea_query::Order::Asc)
        .order_by(Alias::new("ver_count"), sea_orm::sea_query::Order::Desc);

    let mut buckets: BTreeMap<String, Vec<(String, u64)>> = BTreeMap::new();
    for row in database.query_all(&query).await? {
        let bucket: String = row.try_get("", "bucket_time")?;
        let version: String = row.try_get("", "ver_name")?;
        let count = u64::try_from(row.try_get::<i64>("", "ver_count").unwrap_or(0)).unwrap_or(0);
        buckets.entry(bucket).or_default().push((version, count));
    }
    Ok(buckets
        .into_iter()
        .map(|(bucket, versions)| {
            let total_events = versions
                .iter()
                .fold(0_u64, |total, (_, count)| total.saturating_add(*count));
            VersionTimelinePoint {
                bucket,
                total_events,
                versions: versions
                    .into_iter()
                    .map(|(version, count)| VersionShare {
                        version,
                        count,
                        percentage: percentage(count, total_events),
                    })
                    .collect(),
            }
        })
        .collect())
}

async fn compute_version_series(
    database: &DatabaseConnection,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since_ts: Option<i64>,
    bucket_expr: &str,
) -> Result<Vec<VersionSeries>, DbErr> {
    if crate::database::version_dimension::supports_daily_projection(bucket_expr)
        && let Some(points) = crate::database::dimension_rollup::event_dimension_timeline_hybrid(
            database,
            application_id,
            environment_id,
            since_ts,
            crate::database::dimension_rollup::DIMENSION_APP_VERSION,
        )
        .await?
    {
        return Ok(
            crate::database::version_dimension::top_series(&points, bucket_expr)
                .into_iter()
                .map(|series| VersionSeries {
                    version: series.version,
                    total_count: series.total,
                    data: series
                        .points
                        .into_iter()
                        .map(|(day, count)| VersionSeriesPoint { day, count })
                        .collect(),
                })
                .collect(),
        );
    }

    let mut top_query = Query::select();
    top_query
        .expr_as(
            Expr::cust("COALESCE(app_version, 'unknown')"),
            Alias::new("ver"),
        )
        .expr_as(
            Func::count(Expr::col(Alias::new("id"))),
            Alias::new("total"),
        )
        .from(Alias::new("events"));
    apply_event_scope(&mut top_query, application_id, environment_id, since_ts);
    top_query
        .group_by_col(Alias::new("ver"))
        .order_by(Alias::new("total"), sea_orm::sea_query::Order::Desc)
        .limit(8);

    let mut top_versions = Vec::new();
    let mut totals = HashMap::new();
    for row in database.query_all(&top_query).await? {
        let version: String = row.try_get("", "ver")?;
        let total = u64::try_from(row.try_get::<i64>("", "total").unwrap_or(0)).unwrap_or(0);
        totals.insert(version.clone(), total);
        top_versions.push(version);
    }
    if top_versions.is_empty() {
        return Ok(Vec::new());
    }

    let version_placeholders = vec!["?"; top_versions.len()].join(",");
    let mut query = Query::select();
    query
        .expr_as(
            Expr::cust(bucket_expr.to_owned()),
            Alias::new("bucket_time"),
        )
        .expr_as(
            Expr::cust("COALESCE(app_version, 'unknown')"),
            Alias::new("ver"),
        )
        .expr_as(Func::count(Expr::col(Alias::new("id"))), Alias::new("cnt"))
        .from(Alias::new("events"))
        .and_where(Expr::cust_with_values(
            format!("COALESCE(app_version, 'unknown') IN ({version_placeholders})"),
            top_versions.iter().cloned(),
        ));
    apply_event_scope(&mut query, application_id, environment_id, since_ts);
    query
        .group_by_col(Alias::new("bucket_time"))
        .group_by_col(Alias::new("ver"))
        .order_by(Alias::new("bucket_time"), sea_orm::sea_query::Order::Asc);

    let mut data: HashMap<String, BTreeMap<String, u64>> = HashMap::new();
    let mut buckets = BTreeMap::<String, ()>::new();
    for row in database.query_all(&query).await? {
        let bucket: String = row.try_get("", "bucket_time")?;
        let version: String = row.try_get("", "ver")?;
        let count = u64::try_from(row.try_get::<i64>("", "cnt").unwrap_or(0)).unwrap_or(0);
        buckets.insert(bucket.clone(), ());
        data.entry(version).or_default().insert(bucket, count);
    }

    Ok(top_versions
        .into_iter()
        .map(|version| VersionSeries {
            total_count: totals.get(&version).copied().unwrap_or(0),
            data: buckets
                .keys()
                .map(|bucket| VersionSeriesPoint {
                    day: bucket.clone(),
                    count: data
                        .get(&version)
                        .and_then(|points| points.get(bucket))
                        .copied()
                        .unwrap_or(0),
                })
                .collect(),
            version,
        })
        .collect())
}

fn apply_event_scope(
    query: &mut sea_orm::sea_query::SelectStatement,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since_ts: Option<i64>,
) {
    if let Some(application_id) = application_id {
        query.and_where(Expr::col(Alias::new("application_id")).eq(application_id));
    }
    if let Some(environment_id) = environment_id {
        query.and_where(Expr::col(Alias::new("environment_id")).eq(environment_id));
    }
    if let Some(since) = since_ts {
        query.and_where(Expr::col(Alias::new("timestamp")).gte(since));
    }
}

async fn device_distribution(
    database: &DatabaseConnection,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since_ts: Option<i64>,
    expression: &str,
    alias: &str,
    limit: Option<u64>,
) -> Result<Vec<DistributionItem>, DbErr> {
    let mut query = Query::select();
    query
        .expr_as(Expr::cust(expression.to_owned()), Alias::new(alias))
        .expr_as(Func::count(Expr::col(Alias::new("id"))), Alias::new("cnt"))
        .from(Alias::new("telemetry_devices"));

    let mut condition = sea_orm::sea_query::Condition::all();
    if let Some(app_id) = application_id {
        condition = condition.add(Expr::col(Alias::new("application_id")).eq(app_id));
    }
    if let Some(env_id) = environment_id {
        condition = condition.add(Expr::col(Alias::new("environment_id")).eq(env_id));
    }
    if let Some(since) = since_ts {
        condition = condition.add(Expr::col(Alias::new("last_seen_at")).gte(since));
    }

    query
        .cond_where(condition)
        .group_by_col(Alias::new(alias))
        .order_by(Alias::new("cnt"), sea_orm::sea_query::Order::Desc);

    if let Some(limit) = limit {
        query.limit(limit);
    }

    let rows = database.query_all(&query).await?;
    let mut items = Vec::with_capacity(rows.len());
    let mut total = 0_u64;
    for row in rows {
        let name: String = row.try_get("", alias)?;
        let count = u64::try_from(row.try_get::<i64>("", "cnt").unwrap_or(0)).unwrap_or(0);
        total = total.saturating_add(count);
        items.push((name, count));
    }

    Ok(items
        .into_iter()
        .map(|(name, count)| DistributionItem {
            name,
            count,
            percentage: percentage(count, total),
        })
        .collect())
}

pub async fn app_version_distribution(
    database: &DatabaseConnection,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since_ts: Option<i64>,
) -> Result<Vec<DistributionItem>, DbErr> {
    device_distribution(
        database,
        application_id,
        environment_id,
        since_ts,
        "COALESCE(NULLIF(TRIM(last_app_version), ''), 'unknown')",
        "ver",
        Some(50),
    )
    .await
}

pub async fn os_family_distribution(
    database: &DatabaseConnection,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since_ts: Option<i64>,
) -> Result<Vec<DistributionItem>, DbErr> {
    let family_expr = "CASE WHEN last_os LIKE 'Windows%' THEN 'Windows' WHEN last_os LIKE 'Linux%' OR last_os LIKE '%Linux%' OR last_os LIKE '%Fedora%' OR last_os LIKE '%Ubuntu%' OR last_os LIKE '%Debian%' OR last_os LIKE '%Arch%' THEN 'Linux' WHEN last_os LIKE 'Mac%' OR last_os LIKE 'Darwin%' OR last_os LIKE 'macOS%' THEN 'macOS' WHEN last_os LIKE 'Android%' THEN 'Android' WHEN last_os LIKE 'iOS%' THEN 'iOS' ELSE COALESCE(last_os, 'Unknown') END";
    device_distribution(
        database,
        application_id,
        environment_id,
        since_ts,
        family_expr,
        "family_name",
        None,
    )
    .await
}

pub async fn operating_systems_distribution(
    database: &DatabaseConnection,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since_ts: Option<i64>,
) -> Result<Vec<DistributionItem>, DbErr> {
    device_distribution(
        database,
        application_id,
        environment_id,
        since_ts,
        "COALESCE(NULLIF(TRIM(last_os), ''), 'unknown')",
        "os_name",
        Some(50),
    )
    .await
}

pub async fn build_distribution(
    database: &DatabaseConnection,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since_ts: Option<i64>,
) -> Result<Vec<DistributionItem>, DbErr> {
    let build_expr = "CASE WHEN last_os LIKE 'Windows % Build %' THEN 'Win ' || SUBSTR(last_os, 9, INSTR(SUBSTR(last_os, 9), ' Build ') - 1) || ' (' || SUBSTR(last_os, INSTR(last_os, 'Build ') + 6) || ')' WHEN last_os LIKE 'Windows %' THEN last_os WHEN last_os LIKE 'Linux (% Linux %)' THEN REPLACE(SUBSTR(last_os, 8, LENGTH(last_os) - 8), ' Linux', '') WHEN last_os LIKE 'Linux (%)' THEN SUBSTR(last_os, 8, LENGTH(last_os) - 8) WHEN last_os LIKE 'Mac OS X %' THEN REPLACE(last_os, 'Mac OS X ', 'macOS ') WHEN last_os LIKE 'Darwin %' THEN REPLACE(last_os, 'Darwin ', 'macOS ') WHEN last_os LIKE 'Android%' THEN last_os WHEN last_os LIKE 'iOS%' THEN last_os ELSE COALESCE(last_os, 'Unknown') END";
    let items = device_distribution(
        database,
        application_id,
        environment_id,
        since_ts,
        build_expr,
        "build_name",
        Some(100),
    )
    .await?;
    let filtered: Vec<_> = items
        .into_iter()
        .filter(|item| !item.name.trim().is_empty() && !item.name.eq_ignore_ascii_case("unknown"))
        .collect();
    let total = filtered
        .iter()
        .fold(0_u64, |sum, item| sum.saturating_add(item.count));
    Ok(filtered
        .into_iter()
        .map(|item| DistributionItem {
            name: item.name,
            count: item.count,
            percentage: percentage(item.count, total),
        })
        .collect())
}

pub async fn system_language_distribution(
    database: &DatabaseConnection,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since_ts: Option<i64>,
) -> Result<Vec<DistributionItem>, DbErr> {
    device_distribution(
        database,
        application_id,
        environment_id,
        since_ts,
        "COALESCE(NULLIF(TRIM(last_system_language), ''), 'unknown')",
        "lang",
        Some(50),
    )
    .await
}

fn percentage(count: u64, total: u64) -> f64 {
    if total == 0 {
        return 0.0;
    }
    (((count as f64 / total as f64) * 100.0) * 10.0).round() / 10.0
}
