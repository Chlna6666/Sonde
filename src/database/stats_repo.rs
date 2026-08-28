use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, DbErr,
    sea_query::{Alias, Expr, ExprTrait, Func, Query},
};
use serde::Serialize;
use std::collections::HashMap;

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

pub async fn overview(
    database: &DatabaseConnection,
    days: Option<u32>,
) -> Result<Overview, DbErr> {
    let now = chrono::Utc::now().timestamp_millis();
    let since_24h = now - 86_400_000;
    let since_7d = now - 7 * 86_400_000;
    let since_30d = now - 30 * 86_400_000;

    let applications = count(database, "applications", None).await?;
    let events_24h = count(database, "events", Some(("timestamp", since_24h))).await?;
    let metrics_24h = count(database, "metric_points", Some(("timestamp", since_24h))).await?;
    let logs_24h = count(database, "logs", Some(("timestamp", since_24h))).await?;
    let errors_24h = filtered_count(database, "logs", since_24h, "level", &["error", "fatal"]).await?;
    let active_users_24h = distinct_users(database, since_24h, None, None).await?;

    let total_users = count_distinct_users(database, None, None, None).await?;
    let dau = active_users_24h;
    let wau = distinct_users(database, since_7d, None, None).await?;
    let mau = distinct_users(database, since_30d, None, None).await?;

    let bucket_expr = time_bucket_expr(database.get_database_backend(), days);
    let (since_ts, prev_since_ts, prev_until_ts) = match days {
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
        Some(d) => (
            Some(now - d as i64 * 86_400_000),
            Some(now - 2 * d as i64 * 86_400_000),
            Some(now - d as i64 * 86_400_000),
        ),
        None => (None, None, None),
    };

    let growth = compute_growth(database, None, None, since_ts, prev_since_ts, prev_until_ts).await?;

    let trend = if let Some(points) =
        super::trend_repo::global_daily_hybrid(database, days, since_ts).await?
    {
        points
            .into_iter()
            .map(|point| DailyTrendPoint {
                day: point.day,
                events: point.events,
                users: point.users,
            })
            .collect()
    } else {
        let mut trend_query = Query::select();
        trend_query
            .expr_as(Expr::cust(bucket_expr.to_string()), Alias::new("bucket_time"))
            .expr_as(
                Func::count(Expr::col(Alias::new("id"))),
                Alias::new("events"),
            )
            .expr_as(
                Expr::cust("COUNT(DISTINCT anonymous_id)"),
                Alias::new("users"),
            )
            .from(Alias::new("events"));
        if let Some(since) = since_ts {
            trend_query.and_where(Expr::col(Alias::new("timestamp")).gte(since));
        }
        trend_query
            .group_by_col(Alias::new("bucket_time"))
            .order_by(Alias::new("bucket_time"), sea_orm::sea_query::Order::Asc);

        let trend_rows = database.query_all(&trend_query).await?;
        let mut trend = Vec::with_capacity(trend_rows.len());
        for row in trend_rows {
            trend.push(DailyTrendPoint {
                day: row.try_get("", "bucket_time")?,
                events: std::cmp::max(row.try_get::<i64>("", "events").unwrap_or(0), 0) as u64,
                users: std::cmp::max(row.try_get::<i64>("", "users").unwrap_or(0), 0) as u64,
            });
        }
        trend
    };

    let total_events = count(database, "events", since_ts.map(|s| ("timestamp", s))).await?;
    let user_growth = compute_user_growth(database, None, None, since_ts, &bucket_expr).await?;
    let version_timeline = compute_version_timeline(database, None, None, since_ts, &bucket_expr).await?;
    let version_series = compute_version_series(database, None, None, since_ts, &bucket_expr).await?;

    let os_dimension = super::dimension_rollup_repo::event_dimension_timeline_hybrid(
        database,
        None,
        None,
        since_ts,
        super::dimension_rollup_repo::DIMENSION_OS,
    )
    .await?;
    let (os_families, operating_systems, build_distribution) = if let Some(points) = os_dimension {
        (
            distribution_items(
                super::dimension_rollup_repo::aggregate_os_families(&points),
                usize::MAX,
            ),
            distribution_items(
                super::dimension_rollup_repo::aggregate_dimension(&points),
                50,
            ),
            distribution_items(
                super::dimension_rollup_repo::aggregate_os_builds(&points),
                100,
            ),
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
    let row = database.query_one(&query).await?;
    Ok(std::cmp::max(
        row.and_then(|value| value.try_get::<i64>("", "total").ok())
            .unwrap_or(0),
        0,
    ) as u64)
}

async fn filtered_count(
    database: &DatabaseConnection,
    table: &str,
    since: i64,
    column: &str,
    values: &[&str],
) -> Result<u64, DbErr> {
    let query = Query::select()
        .expr_as(
            Func::count(Expr::col(Alias::new("id"))),
            Alias::new("total"),
        )
        .from(Alias::new(table))
        .and_where(Expr::col(Alias::new("timestamp")).gte(since))
        .and_where(Expr::col(Alias::new(column)).is_in(values.iter().copied()))
        .to_owned();
    let row = database.query_one(&query).await?;
    Ok(std::cmp::max(
        row.and_then(|value| value.try_get::<i64>("", "total").ok())
            .unwrap_or(0),
        0,
    ) as u64)
}

async fn distinct_users(
    database: &DatabaseConnection,
    since: i64,
    app_id: Option<&str>,
    env_id: Option<&str>,
) -> Result<u64, DbErr> {
    super::user_rollup_repo::unique_users_hybrid(database, app_id, env_id, Some(since), None).await
}

async fn count_distinct_users(
    database: &DatabaseConnection,
    app_id: Option<&str>,
    env_id: Option<&str>,
    since: Option<i64>,
) -> Result<u64, DbErr> {
    super::user_rollup_repo::unique_users_hybrid(database, app_id, env_id, since, None).await
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
    let (since_ts, prev_since_ts, prev_until_ts) = match days {
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
        Some(d) => (
            Some(now - d as i64 * 86_400_000),
            Some(now - 2 * d as i64 * 86_400_000),
            Some(now - d as i64 * 86_400_000),
        ),
        None => (None, None, None),
    };

    let mut total_events_query = Query::select();
    total_events_query
        .expr_as(
            Func::count(Expr::col(Alias::new("id"))),
            Alias::new("total"),
        )
        .from(Alias::new("events"))
        .and_where(Expr::col(Alias::new("application_id")).eq(application_id));
    if let Some(env_id) = environment_id {
        total_events_query.and_where(Expr::col(Alias::new("environment_id")).eq(env_id));
    }
    if let Some(since) = since_ts {
        total_events_query.and_where(Expr::col(Alias::new("timestamp")).gte(since));
    }
    let total_events = std::cmp::max(
        database
            .query_one(&total_events_query)
            .await?
            .and_then(|r| r.try_get::<i64>("", "total").ok())
            .unwrap_or(0),
        0,
    ) as u64;

    let active_users = count_distinct_users(database, Some(application_id), environment_id, since_ts).await?;
    let total_users = count_distinct_users(database, Some(application_id), environment_id, None).await?;
    let dau = distinct_users(database, since_24h, Some(application_id), environment_id).await?;
    let wau = distinct_users(database, since_7d, Some(application_id), environment_id).await?;
    let mau = distinct_users(database, since_30d, Some(application_id), environment_id).await?;

    let mut errors_query = Query::select();
    errors_query
        .expr_as(
            Func::count(Expr::col(Alias::new("id"))),
            Alias::new("total"),
        )
        .from(Alias::new("logs"))
        .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
        .and_where(Expr::col(Alias::new("level")).is_in(["error", "fatal"]));
    if let Some(env_id) = environment_id {
        errors_query.and_where(Expr::col(Alias::new("environment_id")).eq(env_id));
    }
    if let Some(since) = since_ts {
        errors_query.and_where(Expr::col(Alias::new("timestamp")).gte(since));
    }
    let total_errors = std::cmp::max(
        database
            .query_one(&errors_query)
            .await?
            .and_then(|r| r.try_get::<i64>("", "total").ok())
            .unwrap_or(0),
        0,
    ) as u64;

    let growth = compute_growth(
        database,
        Some(application_id),
        environment_id,
        since_ts,
        prev_since_ts,
        prev_until_ts,
    )
    .await?;

    let trend = if let Some(points) = super::trend_repo::application_daily_hybrid(
        database,
        application_id,
        environment_id,
        days,
        since_ts,
    )
    .await?
    {
        points
            .into_iter()
            .map(|point| DailyTrendPoint {
                day: point.day,
                events: point.events,
                users: point.users,
            })
            .collect()
    } else {
        let mut trend_query = Query::select();
        trend_query
            .expr_as(Expr::cust(bucket_expr.to_string()), Alias::new("bucket_time"))
            .expr_as(
                Func::count(Expr::col(Alias::new("id"))),
                Alias::new("events"),
            )
            .expr_as(
                Expr::cust("COUNT(DISTINCT anonymous_id)"),
                Alias::new("users"),
            )
            .from(Alias::new("events"))
            .and_where(Expr::col(Alias::new("application_id")).eq(application_id));
        if let Some(env_id) = environment_id {
            trend_query.and_where(Expr::col(Alias::new("environment_id")).eq(env_id));
        }
        if let Some(since) = since_ts {
            trend_query.and_where(Expr::col(Alias::new("timestamp")).gte(since));
        }
        trend_query
            .group_by_col(Alias::new("bucket_time"))
            .order_by(Alias::new("bucket_time"), sea_orm::sea_query::Order::Asc);
        let trend_rows = database.query_all(&trend_query).await?;
        let mut trend = Vec::with_capacity(trend_rows.len());
        for row in trend_rows {
            trend.push(DailyTrendPoint {
                day: row.try_get("", "bucket_time")?,
                events: std::cmp::max(row.try_get::<i64>("", "events").unwrap_or(0), 0) as u64,
                users: std::cmp::max(row.try_get::<i64>("", "users").unwrap_or(0), 0) as u64,
            });
        }
        trend
    };

    let days_count = std::cmp::max(trend.len(), 1) as u64;
    let avg_daily_events = total_events / days_count;

    let user_growth = compute_user_growth(
        database,
        Some(application_id),
        environment_id,
        since_ts,
        &bucket_expr,
    )
    .await?;

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

    let app_version_dimension = super::dimension_rollup_repo::event_dimension_timeline_hybrid(
        database,
        Some(application_id),
        environment_id,
        since_ts,
        super::dimension_rollup_repo::DIMENSION_APP_VERSION,
    )
    .await?;
    let app_versions = if let Some(points) = app_version_dimension {
        distribution_items(
            super::dimension_rollup_repo::aggregate_dimension(&points),
            50,
        )
    } else {
        distribution(
            database,
            application_id,
            environment_id,
            since_ts,
            "app_version",
            total_events,
        )
        .await?
    };

    let launcher_version_dimension =
        super::dimension_rollup_repo::event_dimension_timeline_hybrid(
            database,
            Some(application_id),
            environment_id,
            since_ts,
            super::dimension_rollup_repo::DIMENSION_LAUNCHER_VERSION,
        )
        .await?;
    let launcher_versions = if let Some(points) = launcher_version_dimension {
        distribution_items(
            super::dimension_rollup_repo::aggregate_dimension(&points),
            50,
        )
    } else {
        distribution(
            database,
            application_id,
            environment_id,
            since_ts,
            "launcher_version",
            total_events,
        )
        .await?
    };

    let os_dimension = super::dimension_rollup_repo::event_dimension_timeline_hybrid(
        database,
        Some(application_id),
        environment_id,
        since_ts,
        super::dimension_rollup_repo::DIMENSION_OS,
    )
    .await?;
    let (os_families, operating_systems, build_distribution) = if let Some(points) = os_dimension {
        (
            distribution_items(
                super::dimension_rollup_repo::aggregate_os_families(&points),
                usize::MAX,
            ),
            distribution_items(
                super::dimension_rollup_repo::aggregate_dimension(&points),
                50,
            ),
            distribution_items(
                super::dimension_rollup_repo::aggregate_os_builds(&points),
                100,
            ),
        )
    } else {
        (
            os_family_distribution(
                database,
                Some(application_id),
                environment_id,
                since_ts,
                total_events,
            )
            .await?,
            distribution_all(
                database,
                Some(application_id),
                environment_id,
                since_ts,
                "os",
                total_events,
            )
            .await?,
            build_distribution(
                database,
                Some(application_id),
                environment_id,
                since_ts,
                total_events,
            )
            .await?,
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
        let mut curr_e_q = Query::select();
        curr_e_q
            .expr_as(Func::count(Expr::col(Alias::new("id"))), Alias::new("total"))
            .from(Alias::new("events"))
            .and_where(Expr::col(Alias::new("timestamp")).gte(since));
        if let Some(id) = application_id {
            curr_e_q.and_where(Expr::col(Alias::new("application_id")).eq(id));
        }
        if let Some(env) = environment_id {
            curr_e_q.and_where(Expr::col(Alias::new("environment_id")).eq(env));
        }
        let curr_events = std::cmp::max(
            database
                .query_one(&curr_e_q)
                .await?
                .and_then(|r| r.try_get::<i64>("", "total").ok())
                .unwrap_or(0),
            0,
        ) as u64;

        let mut prev_e_q = Query::select();
        prev_e_q
            .expr_as(Func::count(Expr::col(Alias::new("id"))), Alias::new("total"))
            .from(Alias::new("events"))
            .and_where(Expr::col(Alias::new("timestamp")).gte(prev_since))
            .and_where(Expr::col(Alias::new("timestamp")).lt(prev_until));
        if let Some(id) = application_id {
            prev_e_q.and_where(Expr::col(Alias::new("application_id")).eq(id));
        }
        if let Some(env) = environment_id {
            prev_e_q.and_where(Expr::col(Alias::new("environment_id")).eq(env));
        }
        let prev_events = std::cmp::max(
            database
                .query_one(&prev_e_q)
                .await?
                .and_then(|r| r.try_get::<i64>("", "total").ok())
                .unwrap_or(0),
            0,
        ) as u64;

        let curr_users = super::user_rollup_repo::unique_users_hybrid(
            database,
            application_id,
            environment_id,
            Some(since),
            None,
        )
        .await?;
        let prev_users = super::user_rollup_repo::unique_users_hybrid(
            database,
            application_id,
            environment_id,
            Some(prev_since),
            Some(prev_until),
        )
        .await?;

        let events_growth_pct = if prev_events > 0 {
            let pct = ((curr_events as f64 - prev_events as f64) / prev_events as f64) * 100.0;
            Some((pct * 10.0).round() / 10.0)
        } else {
            None
        };

        let users_growth_pct = if prev_users > 0 {
            let pct = ((curr_users as f64 - prev_users as f64) / prev_users as f64) * 100.0;
            Some((pct * 10.0).round() / 10.0)
        } else {
            None
        };

        let mut new_u_q = Query::select();
        new_u_q
            .expr_as(
                Func::count(Expr::col(Alias::new("anonymous_id"))),
                Alias::new("total"),
            )
            .from_subquery(
                Query::select()
                    .column(Alias::new("anonymous_id"))
                    .expr_as(
                        Func::min(Expr::col(Alias::new("timestamp"))),
                        Alias::new("min_ts"),
                    )
                    .from(Alias::new("events"))
                    .and_where(Expr::col(Alias::new("anonymous_id")).is_not_null())
                    .apply_if(application_id, |q, id| {
                        q.and_where(Expr::col(Alias::new("application_id")).eq(id));
                    })
                    .apply_if(environment_id, |q, env| {
                        q.and_where(Expr::col(Alias::new("environment_id")).eq(env));
                    })
                    .group_by_col(Alias::new("anonymous_id"))
                    .take(),
                Alias::new("first_seen"),
            )
            .and_where(Expr::col(Alias::new("min_ts")).gte(since));
        let new_users = std::cmp::max(
            database
                .query_one(&new_u_q)
                .await?
                .and_then(|r| r.try_get::<i64>("", "total").ok())
                .unwrap_or(0),
            0,
        ) as u64;

        let returning_users = curr_users.saturating_sub(new_users);

        Ok(GrowthMetrics {
            events_growth_pct,
            users_growth_pct,
            new_users,
            returning_users,
        })
    } else {
        let total_users = count_distinct_users(database, application_id, environment_id, None).await?;
        Ok(GrowthMetrics {
            events_growth_pct: None,
            users_growth_pct: None,
            new_users: total_users,
            returning_users: 0,
        })
    }
}

async fn compute_user_growth(
    database: &DatabaseConnection,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since_ts: Option<i64>,
    bucket_expr: &str,
) -> Result<Vec<UserGrowthPoint>, DbErr> {
    let mut fs_q = Query::select();
    fs_q.column(Alias::new("anonymous_id"))
        .expr_as(
            Func::min(Expr::col(Alias::new("timestamp"))),
            Alias::new("first_ts"),
        )
        .expr_as(
            Expr::cust(bucket_expr.to_string()),
            Alias::new("first_bucket"),
        )
        .from(Alias::new("events"))
        .and_where(Expr::col(Alias::new("anonymous_id")).is_not_null());
    if let Some(id) = application_id {
        fs_q.and_where(Expr::col(Alias::new("application_id")).eq(id));
    }
    if let Some(env) = environment_id {
        fs_q.and_where(Expr::col(Alias::new("environment_id")).eq(env));
    }
    if let Some(since) = since_ts {
        fs_q.and_where(Expr::col(Alias::new("timestamp")).gte(since));
    }
    fs_q.group_by_col(Alias::new("anonymous_id"));

    let mut new_users_by_bucket: HashMap<String, u64> = HashMap::new();
    let fs_rows = database.query_all(&fs_q).await?;
    for row in fs_rows {
        if let Ok(b) = row.try_get::<String>("", "first_bucket") {
            *new_users_by_bucket.entry(b).or_insert(0) += 1;
        }
    }

    let mut act_q = Query::select();
    act_q
        .expr_as(Expr::cust(bucket_expr.to_string()), Alias::new("bucket_time"))
        .expr_as(
            Expr::cust("COUNT(DISTINCT anonymous_id)"),
            Alias::new("active_users"),
        )
        .from(Alias::new("events"))
        .and_where(Expr::col(Alias::new("anonymous_id")).is_not_null());
    if let Some(id) = application_id {
        act_q.and_where(Expr::col(Alias::new("application_id")).eq(id));
    }
    if let Some(env) = environment_id {
        act_q.and_where(Expr::col(Alias::new("environment_id")).eq(env));
    }
    if let Some(since) = since_ts {
        act_q.and_where(Expr::col(Alias::new("timestamp")).gte(since));
    }
    act_q
        .group_by_col(Alias::new("bucket_time"))
        .order_by(Alias::new("bucket_time"), sea_orm::sea_query::Order::Asc);

    let act_rows = database.query_all(&act_q).await?;
    let mut points = Vec::with_capacity(act_rows.len());
    let mut cumulative = 0u64;

    for row in act_rows {
        let bucket: String = row.try_get("", "bucket_time")?;
        let active = std::cmp::max(
            row.try_get::<i64>("", "active_users").unwrap_or(0),
            0,
        ) as u64;
        let new_u = new_users_by_bucket.get(&bucket).copied().unwrap_or(0);
        cumulative += new_u;
        points.push(UserGrowthPoint {
            bucket,
            new_users: new_u,
            cumulative_users: cumulative,
            active_users: active,
        });
    }

    Ok(points)
}

async fn compute_version_timeline(
    database: &DatabaseConnection,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since_ts: Option<i64>,
    bucket_expr: &str,
) -> Result<Vec<VersionTimelinePoint>, DbErr> {
    if super::version_dimension_repo::supports_daily_projection(bucket_expr) {
        if let Some(points) = super::dimension_rollup_repo::event_dimension_timeline_hybrid(
            database,
            application_id,
            environment_id,
            since_ts,
            super::dimension_rollup_repo::DIMENSION_APP_VERSION,
        )
        .await?
        {
            return Ok(super::version_dimension_repo::timeline(&points, bucket_expr)
                .into_iter()
                .map(|bucket| {
                    let versions = bucket
                        .versions
                        .into_iter()
                        .map(|(version, count)| {
                            let percentage = if bucket.total > 0 {
                                ((count as f64 / bucket.total as f64) * 1000.0).round() / 10.0
                            } else {
                                0.0
                            };
                            VersionShare {
                                version,
                                count,
                                percentage,
                            }
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
        .expr_as(Expr::cust(bucket_expr.to_string()), Alias::new("bucket_time"))
        .expr_as(
            Expr::cust("COALESCE(app_version, 'unknown')"),
            Alias::new("ver_name"),
        )
        .expr_as(
            Func::count(Expr::col(Alias::new("id"))),
            Alias::new("ver_count"),
        )
        .from(Alias::new("events"));
    if let Some(id) = application_id {
        query.and_where(Expr::col(Alias::new("application_id")).eq(id));
    }
    if let Some(env) = environment_id {
        query.and_where(Expr::col(Alias::new("environment_id")).eq(env));
    }
    if let Some(since) = since_ts {
        query.and_where(Expr::col(Alias::new("timestamp")).gte(since));
    }
    query
        .group_by_col(Alias::new("bucket_time"))
        .group_by_col(Alias::new("ver_name"))
        .order_by(Alias::new("bucket_time"), sea_orm::sea_query::Order::Asc)
        .order_by(Alias::new("ver_count"), sea_orm::sea_query::Order::Desc);

    let rows = database.query_all(&query).await?;
    let mut bucket_map: HashMap<String, Vec<(String, u64)>> = HashMap::new();
    let mut bucket_order: Vec<String> = Vec::new();

    for row in rows {
        let bucket: String = row.try_get("", "bucket_time")?;
        let ver: String = row.try_get("", "ver_name")?;
        let count = std::cmp::max(row.try_get::<i64>("", "ver_count").unwrap_or(0), 0) as u64;

        if !bucket_map.contains_key(&bucket) {
            bucket_order.push(bucket.clone());
        }
        bucket_map.entry(bucket).or_default().push((ver, count));
    }

    let mut result = Vec::with_capacity(bucket_order.len());
    for bucket in bucket_order {
        if let Some(list) = bucket_map.remove(&bucket) {
            let total_events: u64 = list.iter().map(|(_, c)| *c).sum();
            let mut versions = Vec::with_capacity(list.len());
            for (version, count) in list {
                let percentage = if total_events > 0 {
                    ((count as f64 / total_events as f64) * 100.0 * 10.0).round() / 10.0
                } else {
                    0.0
                };
                versions.push(VersionShare {
                    version,
                    count,
                    percentage,
                });
            }
            result.push(VersionTimelinePoint {
                bucket,
                total_events,
                versions,
            });
        }
    }

    Ok(result)
}

async fn compute_version_series(
    database: &DatabaseConnection,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since_ts: Option<i64>,
    bucket_expr: &str,
) -> Result<Vec<VersionSeries>, DbErr> {
    if super::version_dimension_repo::supports_daily_projection(bucket_expr) {
        if let Some(points) = super::dimension_rollup_repo::event_dimension_timeline_hybrid(
            database,
            application_id,
            environment_id,
            since_ts,
            super::dimension_rollup_repo::DIMENSION_APP_VERSION,
        )
        .await?
        {
            return Ok(super::version_dimension_repo::top_series(&points, bucket_expr)
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

    let mut top_v_q = Query::select();
    top_v_q
        .expr_as(
            Expr::cust("COALESCE(app_version, 'unknown')"),
            Alias::new("ver"),
        )
        .expr_as(
            Func::count(Expr::col(Alias::new("id"))),
            Alias::new("total"),
        )
        .from(Alias::new("events"));
    if let Some(id) = application_id {
        top_v_q.and_where(Expr::col(Alias::new("application_id")).eq(id));
    }
    if let Some(env) = environment_id {
        top_v_q.and_where(Expr::col(Alias::new("environment_id")).eq(env));
    }
    if let Some(since) = since_ts {
        top_v_q.and_where(Expr::col(Alias::new("timestamp")).gte(since));
    }
    top_v_q
        .group_by_col(Alias::new("ver"))
        .order_by(Alias::new("total"), sea_orm::sea_query::Order::Desc)
        .limit(8);

    let top_rows = database.query_all(&top_v_q).await?;
    let mut top_versions = Vec::with_capacity(top_rows.len());
    let mut version_totals = HashMap::new();
    for r in top_rows {
        let v: String = r.try_get("", "ver")?;
        let tot = std::cmp::max(r.try_get::<i64>("", "total").unwrap_or(0), 0) as u64;
        version_totals.insert(v.clone(), tot);
        top_versions.push(v);
    }

    if top_versions.is_empty() {
        return Ok(Vec::new());
    }

    let mut query = Query::select();
    query
        .expr_as(Expr::cust(bucket_expr.to_string()), Alias::new("bucket_time"))
        .expr_as(
            Expr::cust("COALESCE(app_version, 'unknown')"),
            Alias::new("ver"),
        )
        .expr_as(
            Func::count(Expr::col(Alias::new("id"))),
            Alias::new("cnt"),
        )
        .from(Alias::new("events"))
        .and_where(Expr::cust(format!(
            "COALESCE(app_version, 'unknown') IN ({})",
            top_versions
                .iter()
                .map(|v| format!("'{}'", v.replace('\'', "''")))
                .collect::<Vec<_>>()
                .join(",")
        )));
    if let Some(id) = application_id {
        query.and_where(Expr::col(Alias::new("application_id")).eq(id));
    }
    if let Some(env) = environment_id {
        query.and_where(Expr::col(Alias::new("environment_id")).eq(env));
    }
    if let Some(since) = since_ts {
        query.and_where(Expr::col(Alias::new("timestamp")).gte(since));
    }
    query
        .group_by_col(Alias::new("bucket_time"))
        .group_by_col(Alias::new("ver"))
        .order_by(Alias::new("bucket_time"), sea_orm::sea_query::Order::Asc);

    let rows = database.query_all(&query).await?;
    let mut version_data: HashMap<String, HashMap<String, u64>> = HashMap::new();
    let mut all_buckets = Vec::new();

    for row in rows {
        let b: String = row.try_get("", "bucket_time")?;
        let v: String = row.try_get("", "ver")?;
        let c = std::cmp::max(row.try_get::<i64>("", "cnt").unwrap_or(0), 0) as u64;

        if !all_buckets.contains(&b) {
            all_buckets.push(b.clone());
        }
        version_data.entry(v).or_default().insert(b, c);
    }

    let mut result = Vec::with_capacity(top_versions.len());
    for v in top_versions {
        let tot = version_totals.get(&v).copied().unwrap_or(0);
        let mut pts = Vec::with_capacity(all_buckets.len());
        if let Some(map) = version_data.get(&v) {
            for b in &all_buckets {
                pts.push(VersionSeriesPoint {
                    day: b.clone(),
                    count: map.get(b).copied().unwrap_or(0),
                });
            }
        }
        result.push(VersionSeries {
            version: v,
            total_count: tot,
            data: pts,
        });
    }

    Ok(result)
}

async fn os_family_distribution(
    database: &DatabaseConnection,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since_ts: Option<i64>,
    total_events: u64,
) -> Result<Vec<DistributionItem>, DbErr> {
    let family_expr = "CASE         WHEN os LIKE 'Windows%' THEN 'Windows'         WHEN os LIKE 'Linux%' OR os LIKE '%Linux%' OR os LIKE '%Fedora%' OR os LIKE '%Ubuntu%' OR os LIKE '%Debian%' OR os LIKE '%Arch%' THEN 'Linux'         WHEN os LIKE 'Mac%' OR os LIKE 'Darwin%' OR os LIKE 'macOS%' THEN 'macOS'         WHEN os LIKE 'Android%' THEN 'Android'         WHEN os LIKE 'iOS%' THEN 'iOS'         ELSE COALESCE(os, 'Unknown')     END";

    let mut query = Query::select();
    query
        .expr_as(Expr::cust(family_expr), Alias::new("family_name"))
        .expr_as(
            Func::count(Expr::col(Alias::new("id"))),
            Alias::new("item_count"),
        )
        .from(Alias::new("events"));
    if let Some(id) = application_id {
        query.and_where(Expr::col(Alias::new("application_id")).eq(id));
    }
    if let Some(env_id) = environment_id {
        query.and_where(Expr::col(Alias::new("environment_id")).eq(env_id));
    }
    if let Some(since) = since_ts {
        query.and_where(Expr::col(Alias::new("timestamp")).gte(since));
    }
    query
        .group_by_col(Alias::new("family_name"))
        .order_by(Alias::new("item_count"), sea_orm::sea_query::Order::Desc);

    let rows = database.query_all(&query).await?;
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let count = std::cmp::max(row.try_get::<i64>("", "item_count").unwrap_or(0), 0) as u64;
        let percentage = if total_events > 0 {
            (count as f64 / total_events as f64) * 100.0
        } else {
            0.0
        };
        items.push(DistributionItem {
            name: row.try_get("", "family_name")?,
            count,
            percentage: (percentage * 10.0).round() / 10.0,
        });
    }
    Ok(items)
}

async fn build_distribution(
    database: &DatabaseConnection,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since_ts: Option<i64>,
    total_events: u64,
) -> Result<Vec<DistributionItem>, DbErr> {
    let build_expr = "CASE         WHEN os LIKE 'Windows % Build %' THEN 'Win ' || SUBSTR(os, 9, INSTR(SUBSTR(os, 9), ' Build ') - 1) || ' (' || SUBSTR(os, INSTR(os, 'Build ') + 6) || ')'         WHEN os LIKE 'Windows %' THEN os         WHEN os LIKE 'Linux (% Linux %)' THEN REPLACE(SUBSTR(os, 8, LENGTH(os) - 8), ' Linux', '')         WHEN os LIKE 'Linux (%)' THEN SUBSTR(os, 8, LENGTH(os) - 8)         WHEN os LIKE 'Mac OS X %' THEN REPLACE(os, 'Mac OS X ', 'macOS ')         WHEN os LIKE 'Darwin %' THEN REPLACE(os, 'Darwin ', 'macOS ')         WHEN os LIKE 'Android%' THEN os         WHEN os LIKE 'iOS%' THEN os         ELSE COALESCE(os, 'Unknown')     END";

    let mut query = Query::select();
    query
        .expr_as(Expr::cust(build_expr), Alias::new("build_name"))
        .expr_as(
            Func::count(Expr::col(Alias::new("id"))),
            Alias::new("item_count"),
        )
        .from(Alias::new("events"));
    if let Some(id) = application_id {
        query.and_where(Expr::col(Alias::new("application_id")).eq(id));
    }
    if let Some(env_id) = environment_id {
        query.and_where(Expr::col(Alias::new("environment_id")).eq(env_id));
    }
    if let Some(since) = since_ts {
        query.and_where(Expr::col(Alias::new("timestamp")).gte(since));
    }
    query
        .group_by_col(Alias::new("build_name"))
        .order_by(Alias::new("item_count"), sea_orm::sea_query::Order::Desc)
        .limit(100);

    let rows = database.query_all(&query).await?;
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let count = std::cmp::max(row.try_get::<i64>("", "item_count").unwrap_or(0), 0) as u64;
        let percentage = if total_events > 0 {
            (count as f64 / total_events as f64) * 100.0
        } else {
            0.0
        };
        items.push(DistributionItem {
            name: row.try_get("", "build_name")?,
            count,
            percentage: (percentage * 10.0).round() / 10.0,
        });
    }
    Ok(items)
}

async fn distribution_all(
    database: &DatabaseConnection,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since_ts: Option<i64>,
    column: &str,
    total_events: u64,
) -> Result<Vec<DistributionItem>, DbErr> {
    let mut query = Query::select();
    query
        .expr_as(
            Expr::cust(format!("COALESCE({column}, 'unknown')")),
            Alias::new("item_name"),
        )
        .expr_as(
            Func::count(Expr::col(Alias::new("id"))),
            Alias::new("item_count"),
        )
        .from(Alias::new("events"));
    if let Some(id) = application_id {
        query.and_where(Expr::col(Alias::new("application_id")).eq(id));
    }
    if let Some(env_id) = environment_id {
        query.and_where(Expr::col(Alias::new("environment_id")).eq(env_id));
    }
    if let Some(since) = since_ts {
        query.and_where(Expr::col(Alias::new("timestamp")).gte(since));
    }
    query
        .group_by_col(Alias::new("item_name"))
        .order_by(Alias::new("item_count"), sea_orm::sea_query::Order::Desc)
        .limit(50);
    let rows = database.query_all(&query).await?;
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let count = std::cmp::max(row.try_get::<i64>("", "item_count").unwrap_or(0), 0) as u64;
        let percentage = if total_events > 0 {
            (count as f64 / total_events as f64) * 100.0
        } else {
            0.0
        };
        items.push(DistributionItem {
            name: row.try_get("", "item_name")?,
            count,
            percentage: (percentage * 10.0).round() / 10.0,
        });
    }
    Ok(items)
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
    counts: Vec<super::dimension_rollup_repo::DimensionCount>,
    limit: usize,
) -> Vec<DistributionItem> {
    let total = counts
        .iter()
        .fold(0_u64, |acc, item| acc.saturating_add(item.count));
    counts
        .into_iter()
        .take(limit)
        .map(|item| {
            let percentage = if total > 0 {
                (item.count as f64 / total as f64) * 100.0
            } else {
                0.0
            };
            DistributionItem {
                name: item.value,
                count: item.count,
                percentage: (percentage * 10.0).round() / 10.0,
            }
        })
        .collect()
}
