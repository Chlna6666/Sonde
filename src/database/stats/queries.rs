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
    pub os_families: Vec<DistributionItem>,
    pub operating_systems: Vec<DistributionItem>,
    pub build_distribution: Vec<DistributionItem>,
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
    pub launcher_versions: Vec<DistributionItem>,
    pub os_families: Vec<DistributionItem>,
    pub operating_systems: Vec<DistributionItem>,
    pub build_distribution: Vec<DistributionItem>,
}

pub async fn overview(database: &DatabaseConnection, days: Option<u32>) -> Result<Overview, DbErr> {
    let now = chrono::Utc::now().timestamp_millis();
    let since_24h = now - 86_400_000;
    let since_7d = now - 7 * 86_400_000;
    let since_30d = now - 30 * 86_400_000;

    let applications = count(database, "applications", None).await?;
    let events_24h = telemetry_count(database, crate::database::telemetry_count::RollupCountKind::Events, None, None, Some(since_24h), None).await?;
    let metrics_24h = telemetry_count(database, crate::database::telemetry_count::RollupCountKind::Metrics, None, None, Some(since_24h), None).await?;
    let logs_24h = telemetry_count(database, crate::database::telemetry_count::RollupCountKind::Logs, None, None, Some(since_24h), None).await?;
    let errors_24h = telemetry_count(database, crate::database::telemetry_count::RollupCountKind::ErrorLogs, None, None, Some(since_24h), None).await?;

    let active_users_24h = distinct_users(database, since_24h, None, None).await?;
    let total_users = count_distinct_users(database, None, None, None).await?;
    let dau = active_users_24h;
    let wau = distinct_users(database, since_7d, None, None).await?;
    let mau = distinct_users(database, since_30d, None, None).await?;

    let bucket_expr = time_bucket_expr(database.get_database_backend(), days);
    let (since_ts, prev_since_ts, prev_until_ts) = statistics_window(now, days);
    let growth = compute_growth(database, None, None, since_ts, prev_since_ts, prev_until_ts).await?;
    let user_growth = compute_user_growth(database, None, None, since_ts, days).await?;
    let event_trend = event_trend(database, None, None, days, since_ts, &bucket_expr).await?;
    let trend = merge_activity_trend(event_trend, &user_growth);

    let total_events = telemetry_count(
        database,
        crate::database::telemetry_count::RollupCountKind::Events,
        None,
        None,
        since_ts,
        None,
    )
    .await?;
    let version_timeline = compute_version_timeline(database, None, None, since_ts, &bucket_expr).await?;
    let version_series = compute_version_series(database, None, None, since_ts, &bucket_expr).await?;

    let os_dimension = crate::database::dimension_rollup::event_dimension_timeline_hybrid(
        database,
        None,
        None,
        since_ts,
        crate::database::dimension_rollup::DIMENSION_OS,
    )
    .await?;
    let (os_families, operating_systems, build_distribution) = if let Some(points) = os_dimension {
        (
            distribution_items(crate::database::dimension_rollup::aggregate_os_families(&points), usize::MAX),
            distribution_items(crate::database::dimension_rollup::aggregate_dimension(&points), 50),
            distribution_items(crate::database::dimension_rollup::aggregate_os_builds(&points), 100),
        )
    } else {
        (
            os_family_distribution(database, None, None, since_ts, total_events).await?,
            distribution_all(database, None, None, since_ts, "os", total_events).await?,
            build_distribution(database, None, None, since_ts, total_events).await?,
        )
    };

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
        os_families,
        operating_systems,
        build_distribution,
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
    let active_users = count_distinct_users(database, Some(application_id), environment_id, since_ts).await?;
    let total_users = count_distinct_users(database, Some(application_id), environment_id, None).await?;
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

    let app_version_dimension = crate::database::dimension_rollup::event_dimension_timeline_hybrid(
        database,
        Some(application_id),
        environment_id,
        since_ts,
        crate::database::dimension_rollup::DIMENSION_APP_VERSION,
    )
    .await?;
    let app_versions = if let Some(points) = app_version_dimension {
        distribution_items(crate::database::dimension_rollup::aggregate_dimension(&points), 50)
    } else {
        distribution(database, application_id, environment_id, since_ts, "app_version", total_events).await?
    };

    let launcher_version_dimension = crate::database::dimension_rollup::event_dimension_timeline_hybrid(
        database,
        Some(application_id),
        environment_id,
        since_ts,
        crate::database::dimension_rollup::DIMENSION_LAUNCHER_VERSION,
    )
    .await?;
    let launcher_versions = if let Some(points) = launcher_version_dimension {
        distribution_items(crate::database::dimension_rollup::aggregate_dimension(&points), 50)
    } else {
        distribution(database, application_id, environment_id, since_ts, "launcher_version", total_events).await?
    };

    let os_dimension = crate::database::dimension_rollup::event_dimension_timeline_hybrid(
        database,
        Some(application_id),
        environment_id,
        since_ts,
        crate::database::dimension_rollup::DIMENSION_OS,
    )
    .await?;
    let (os_families, operating_systems, build_distribution) = if let Some(points) = os_dimension {
        (
            distribution_items(crate::database::dimension_rollup::aggregate_os_families(&points), usize::MAX),
            distribution_items(crate::database::dimension_rollup::aggregate_dimension(&points), 50),
            distribution_items(crate::database::dimension_rollup::aggregate_os_builds(&points), 100),
        )
    } else {
        (
            os_family_distribution(database, Some(application_id), environment_id, since_ts, total_events).await?,
            distribution_all(database, Some(application_id), environment_id, since_ts, "os", total_events).await?,
            build_distribution(database, Some(application_id), environment_id, since_ts, total_events).await?,
        )
    };

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
        launcher_versions,
        os_families,
        operating_systems,
        build_distribution,
    })
}

fn statistics_window(now: i64, days: Option<u32>) -> (Option<i64>, Option<i64>, Option<i64>) {
    match days {
        Some(1) => (Some(now - 86_400_000), Some(now - 2 * 86_400_000), Some(now - 86_400_000)),
        Some(7) => (Some(now - 7 * 86_400_000), Some(now - 14 * 86_400_000), Some(now - 7 * 86_400_000)),
        Some(30) => (Some(now - 30 * 86_400_000), Some(now - 60 * 86_400_000), Some(now - 30 * 86_400_000)),
        Some(365) => (Some(now - 365 * 86_400_000), Some(now - 2 * 365 * 86_400_000), Some(now - 365 * 86_400_000)),
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
        .expr_as(Func::count(Expr::col(Alias::new("id"))), Alias::new("total"))
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
        Some(since) => crate::database::device_activity::unique_devices(
            database,
            application_id,
            environment_id,
            Some(since),
            None,
        )
        .await,
        None => crate::database::device_activity::total_devices(
            database,
            application_id,
            environment_id,
        )
        .await,
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
    if let (Some(since), Some(prev_since), Some(prev_until)) = (since_ts, prev_since_ts, prev_until_ts) {
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
        Some(application_id) => crate::database::trends::application_daily_hybrid(
            database,
            application_id,
            environment_id,
            days,
            since_ts,
        )
        .await?,
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
        .expr_as(Expr::cust(bucket_expr.to_owned()), Alias::new("bucket_time"))
        .expr_as(Func::count(Expr::col(Alias::new("id"))), Alias::new("events"))
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
    if crate::database::version_dimension::supports_daily_projection(bucket_expr) {
        if let Some(points) = crate::database::dimension_rollup::event_dimension_timeline_hybrid(
            database,
            application_id,
            environment_id,
            since_ts,
            crate::database::dimension_rollup::DIMENSION_APP_VERSION,
        )
        .await?
        {
            return Ok(crate::database::version_dimension::timeline(&points, bucket_expr)
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
                .collect());
        }
    }

    let mut query = Query::select();
    query
        .expr_as(Expr::cust(bucket_expr.to_owned()), Alias::new("bucket_time"))
        .expr_as(Expr::cust("COALESCE(app_version, 'unknown')"), Alias::new("ver_name"))
        .expr_as(Func::count(Expr::col(Alias::new("id"))), Alias::new("ver_count"))
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
            let total_events = versions.iter().fold(0_u64, |total, (_, count)| total.saturating_add(*count));
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
    if crate::database::version_dimension::supports_daily_projection(bucket_expr) {
        if let Some(points) = crate::database::dimension_rollup::event_dimension_timeline_hybrid(
            database,
            application_id,
            environment_id,
            since_ts,
            crate::database::dimension_rollup::DIMENSION_APP_VERSION,
        )
        .await?
        {
            return Ok(crate::database::version_dimension::top_series(&points, bucket_expr)
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
                .collect());
        }
    }

    let mut top_query = Query::select();
    top_query
        .expr_as(Expr::cust("COALESCE(app_version, 'unknown')"), Alias::new("ver"))
        .expr_as(Func::count(Expr::col(Alias::new("id"))), Alias::new("total"))
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

    let escaped_versions = top_versions
        .iter()
        .map(|value| format!("'{}'", value.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(",");
    let mut query = Query::select();
    query
        .expr_as(Expr::cust(bucket_expr.to_owned()), Alias::new("bucket_time"))
        .expr_as(Expr::cust("COALESCE(app_version, 'unknown')"), Alias::new("ver"))
        .expr_as(Func::count(Expr::col(Alias::new("id"))), Alias::new("cnt"))
        .from(Alias::new("events"))
        .and_where(Expr::cust(format!("COALESCE(app_version, 'unknown') IN ({escaped_versions})")));
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

async fn os_family_distribution(
    database: &DatabaseConnection,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since_ts: Option<i64>,
    total_events: u64,
) -> Result<Vec<DistributionItem>, DbErr> {
    let family_expr = "CASE WHEN os LIKE 'Windows%' THEN 'Windows' WHEN os LIKE 'Linux%' OR os LIKE '%Linux%' OR os LIKE '%Fedora%' OR os LIKE '%Ubuntu%' OR os LIKE '%Debian%' OR os LIKE '%Arch%' THEN 'Linux' WHEN os LIKE 'Mac%' OR os LIKE 'Darwin%' OR os LIKE 'macOS%' THEN 'macOS' WHEN os LIKE 'Android%' THEN 'Android' WHEN os LIKE 'iOS%' THEN 'iOS' ELSE COALESCE(os, 'Unknown') END";
    grouped_distribution(database, application_id, environment_id, since_ts, family_expr, "family_name", total_events, None).await
}

async fn build_distribution(
    database: &DatabaseConnection,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since_ts: Option<i64>,
    total_events: u64,
) -> Result<Vec<DistributionItem>, DbErr> {
    let build_expr = "CASE WHEN os LIKE 'Windows % Build %' THEN 'Win ' || SUBSTR(os, 9, INSTR(SUBSTR(os, 9), ' Build ') - 1) || ' (' || SUBSTR(os, INSTR(os, 'Build ') + 6) || ')' WHEN os LIKE 'Windows %' THEN os WHEN os LIKE 'Linux (% Linux %)' THEN REPLACE(SUBSTR(os, 8, LENGTH(os) - 8), ' Linux', '') WHEN os LIKE 'Linux (%)' THEN SUBSTR(os, 8, LENGTH(os) - 8) WHEN os LIKE 'Mac OS X %' THEN REPLACE(os, 'Mac OS X ', 'macOS ') WHEN os LIKE 'Darwin %' THEN REPLACE(os, 'Darwin ', 'macOS ') WHEN os LIKE 'Android%' THEN os WHEN os LIKE 'iOS%' THEN os ELSE COALESCE(os, 'Unknown') END";
    grouped_distribution(database, application_id, environment_id, since_ts, build_expr, "build_name", total_events, Some(100)).await
}

async fn distribution_all(
    database: &DatabaseConnection,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since_ts: Option<i64>,
    column: &str,
    total_events: u64,
) -> Result<Vec<DistributionItem>, DbErr> {
    grouped_distribution(
        database,
        application_id,
        environment_id,
        since_ts,
        &format!("COALESCE({column}, 'unknown')"),
        "item_name",
        total_events,
        Some(50),
    )
    .await
}

async fn grouped_distribution(
    database: &DatabaseConnection,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since_ts: Option<i64>,
    expression: &str,
    alias: &str,
    total_events: u64,
    limit: Option<u64>,
) -> Result<Vec<DistributionItem>, DbErr> {
    let mut query = Query::select();
    query
        .expr_as(Expr::cust(expression.to_owned()), Alias::new(alias))
        .expr_as(Func::count(Expr::col(Alias::new("id"))), Alias::new("item_count"))
        .from(Alias::new("events"));
    apply_event_scope(&mut query, application_id, environment_id, since_ts);
    query
        .group_by_col(Alias::new(alias))
        .order_by(Alias::new("item_count"), sea_orm::sea_query::Order::Desc);
    if let Some(limit) = limit {
        query.limit(limit);
    }

    database
        .query_all(&query)
        .await?
        .into_iter()
        .map(|row| {
            let count = u64::try_from(row.try_get::<i64>("", "item_count").unwrap_or(0)).unwrap_or(0);
            Ok(DistributionItem {
                name: row.try_get("", alias)?,
                count,
                percentage: percentage(count, total_events),
            })
        })
        .collect()
}

async fn distribution(
    database: &DatabaseConnection,
    application_id: &str,
    environment_id: Option<&str>,
    since_ts: Option<i64>,
    column: &str,
    total_events: u64,
) -> Result<Vec<DistributionItem>, DbErr> {
    distribution_all(
        database,
        Some(application_id),
        environment_id,
        since_ts,
        column,
        total_events,
    )
    .await
}

fn distribution_items(
    counts: Vec<crate::database::dimension_rollup::DimensionCount>,
    limit: usize,
) -> Vec<DistributionItem> {
    let total = counts.iter().fold(0_u64, |sum, item| sum.saturating_add(item.count));
    counts
        .into_iter()
        .take(limit)
        .map(|item| DistributionItem {
            name: item.value,
            count: item.count,
            percentage: percentage(item.count, total),
        })
        .collect()
}

fn percentage(count: u64, total: u64) -> f64 {
    if total == 0 {
        return 0.0;
    }
    (((count as f64 / total as f64) * 100.0) * 10.0).round() / 10.0
}
