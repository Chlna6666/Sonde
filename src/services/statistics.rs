use crate::{
    database::{device_activity, stats},
    error::AppError,
    services::authentication::AuthenticatedUser,
    state::InstalledState,
};

pub use super::statistics_models::{
    AppStatsOverview, AppTelemetryStats, DailyTrendPoint, DistributionItem, GrowthMetrics, Overview,
    UserGrowthPoint, VersionSeries, VersionSeriesPoint, VersionShare, VersionTimelinePoint,
};

const MAX_STATISTICS_DAYS: u32 = 730;

pub fn validate_days(days: Option<u32>) -> Result<Option<u32>, AppError> {
    match days {
        Some(0) => Err(AppError::Validation(
            "statistics days must be greater than zero".into(),
        )),
        Some(days) if days > MAX_STATISTICS_DAYS => Err(AppError::Validation(format!(
            "statistics days must not exceed {MAX_STATISTICS_DAYS}"
        ))),
        _ => Ok(days),
    }
}

pub async fn overview(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    days: Option<u32>,
) -> Result<Overview, AppError> {
    user.require("telemetry.read", None)?;
    let days = validate_days(days)?;
    Ok(map_overview(stats::overview(&installed.database, days).await?))
}

pub async fn application_stats(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    application_id: &str,
    environment_id: Option<&str>,
    days: Option<u32>,
) -> Result<AppTelemetryStats, AppError> {
    user.require("telemetry.read", Some(application_id))?;
    query_application_stats(installed, application_id, environment_id, days).await
}

pub async fn public_application_stats(
    installed: &InstalledState,
    application_id: &str,
    days: Option<u32>,
) -> Result<AppTelemetryStats, AppError> {
    query_application_stats(installed, application_id, None, days).await
}

async fn query_application_stats(
    installed: &InstalledState,
    application_id: &str,
    environment_id: Option<&str>,
    days: Option<u32>,
) -> Result<AppTelemetryStats, AppError> {
    let days = validate_days(days)?;
    let mut record =
        stats::application_stats(&installed.database, application_id, environment_id, days).await?;
    let calendar_days = statistics_calendar_days(
        installed,
        application_id,
        environment_id,
        days,
    )
    .await?;
    record.overview.avg_daily_events = record.overview.total_events / calendar_days;
    Ok(map_application_stats(record))
}

async fn statistics_calendar_days(
    installed: &InstalledState,
    application_id: &str,
    environment_id: Option<&str>,
    days: Option<u32>,
) -> Result<u64, AppError> {
    if let Some(days) = days {
        return Ok(u64::from(std::cmp::max(days, 1)));
    }

    let activity = device_activity::daily_activity(
        &installed.database,
        Some(application_id),
        environment_id,
        None,
    )
    .await?;
    let Some(first) = activity.first() else {
        return Ok(1);
    };
    let Some(last) = activity.last() else {
        return Ok(1);
    };
    let first = chrono::NaiveDate::parse_from_str(&first.day, "%Y-%m-%d")
        .map_err(|error| AppError::internal("parse first device activity day", error))?;
    let last = chrono::NaiveDate::parse_from_str(&last.day, "%Y-%m-%d")
        .map_err(|error| AppError::internal("parse last device activity day", error))?;
    let span = last.signed_duration_since(first).num_days().saturating_add(1);
    u64::try_from(std::cmp::max(span, 1))
        .map_err(|error| AppError::internal("convert statistics calendar day span", error))
}

fn map_overview(record: stats::Overview) -> Overview {
    Overview {
        applications: record.applications,
        events_24h: record.events_24h,
        metrics_24h: record.metrics_24h,
        logs_24h: record.logs_24h,
        errors_24h: record.errors_24h,
        active_users_24h: record.active_users_24h,
        total_users: record.total_users,
        dau: record.dau,
        wau: record.wau,
        mau: record.mau,
        growth: map_growth(record.growth),
        trend: record.trend.into_iter().map(map_trend).collect(),
        user_growth: record
            .user_growth
            .into_iter()
            .map(map_user_growth)
            .collect(),
        version_timeline: record
            .version_timeline
            .into_iter()
            .map(map_version_timeline)
            .collect(),
        version_series: record
            .version_series
            .into_iter()
            .map(map_version_series)
            .collect(),
        os_families: record
            .os_families
            .into_iter()
            .map(map_distribution)
            .collect(),
        operating_systems: record
            .operating_systems
            .into_iter()
            .map(map_distribution)
            .collect(),
        build_distribution: record
            .build_distribution
            .into_iter()
            .map(map_distribution)
            .collect(),
    }
}

fn map_application_stats(record: stats::AppTelemetryStats) -> AppTelemetryStats {
    AppTelemetryStats {
        overview: map_app_overview(record.overview),
        growth: map_growth(record.growth),
        trend: record.trend.into_iter().map(map_trend).collect(),
        user_growth: record
            .user_growth
            .into_iter()
            .map(map_user_growth)
            .collect(),
        version_timeline: record
            .version_timeline
            .into_iter()
            .map(map_version_timeline)
            .collect(),
        version_series: record
            .version_series
            .into_iter()
            .map(map_version_series)
            .collect(),
        app_versions: record
            .app_versions
            .into_iter()
            .map(map_distribution)
            .collect(),
        launcher_versions: record
            .launcher_versions
            .into_iter()
            .map(map_distribution)
            .collect(),
        os_families: record
            .os_families
            .into_iter()
            .map(map_distribution)
            .collect(),
        operating_systems: record
            .operating_systems
            .into_iter()
            .map(map_distribution)
            .collect(),
        build_distribution: record
            .build_distribution
            .into_iter()
            .map(map_distribution)
            .collect(),
    }
}

fn map_app_overview(record: stats::AppStatsOverview) -> AppStatsOverview {
    AppStatsOverview {
        total_events: record.total_events,
        active_users: record.active_users,
        total_errors: record.total_errors,
        avg_daily_events: record.avg_daily_events,
        total_users: record.total_users,
        dau: record.dau,
        wau: record.wau,
        mau: record.mau,
    }
}

fn map_growth(record: stats::GrowthMetrics) -> GrowthMetrics {
    GrowthMetrics {
        events_growth_pct: record.events_growth_pct,
        users_growth_pct: record.users_growth_pct,
        new_users: record.new_users,
        returning_users: record.returning_users,
    }
}

fn map_trend(record: stats::DailyTrendPoint) -> DailyTrendPoint {
    DailyTrendPoint {
        day: record.day,
        events: record.events,
        users: record.users,
    }
}

fn map_user_growth(record: stats::UserGrowthPoint) -> UserGrowthPoint {
    UserGrowthPoint {
        bucket: record.bucket,
        new_users: record.new_users,
        cumulative_users: record.cumulative_users,
        active_users: record.active_users,
    }
}

fn map_version_timeline(record: stats::VersionTimelinePoint) -> VersionTimelinePoint {
    VersionTimelinePoint {
        bucket: record.bucket,
        total_events: record.total_events,
        versions: record.versions.into_iter().map(map_version_share).collect(),
    }
}

fn map_version_share(record: stats::VersionShare) -> VersionShare {
    VersionShare {
        version: record.version,
        count: record.count,
        percentage: record.percentage,
    }
}

fn map_version_series(record: stats::VersionSeries) -> VersionSeries {
    VersionSeries {
        version: record.version,
        total_count: record.total_count,
        data: record
            .data
            .into_iter()
            .map(map_version_series_point)
            .collect(),
    }
}

fn map_version_series_point(record: stats::VersionSeriesPoint) -> VersionSeriesPoint {
    VersionSeriesPoint {
        day: record.day,
        count: record.count,
    }
}

fn map_distribution(record: stats::DistributionItem) -> DistributionItem {
    DistributionItem {
        name: record.name,
        count: record.count,
        percentage: record.percentage,
    }
}

#[cfg(test)]
mod tests {
    use super::validate_days;

    #[test]
    fn statistics_window_is_bounded() {
        assert!(validate_days(Some(0)).is_err());
        assert!(validate_days(Some(731)).is_err());
        assert_eq!(validate_days(Some(365)).ok().flatten(), Some(365));
    }
}
