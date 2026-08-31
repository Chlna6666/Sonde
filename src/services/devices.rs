use crate::{
    database::device_query,
    error::AppError,
    services::{
        applications,
        authentication::AuthenticatedUser,
        device_models::{
            DevicePage, DeviceQuery, DeviceRiskFilter, DeviceRiskLevel, DeviceSecuritySummary,
            DeviceStatus, DeviceStatusFilter, DeviceSummary,
        },
    },
    state::InstalledState,
};

pub use super::device_models::{DeviceRiskFilter as RiskFilter, DeviceStatusFilter as StatusFilter};
pub use super::device_models::DeviceQuery as Query;

const ACTIVE_WINDOW_MILLIS: i64 = 15 * 60_000;
const RECENT_WINDOW_MILLIS: i64 = 24 * 60 * 60_000;

pub async fn list(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    query: DeviceQuery,
) -> Result<DevicePage, AppError> {
    applications::ensure_app_access(&installed.database, user, &query.application_id, false).await?;

    let now = chrono::Utc::now().timestamp_millis();
    let active_since = now.saturating_sub(ACTIVE_WINDOW_MILLIS);
    let recent_since = now.saturating_sub(RECENT_WINDOW_MILLIS);
    let (last_seen_from, last_seen_to) = status_window(query.status, active_since, recent_since);
    let (risk_min, risk_max) = risk_window(query.risk, query.min_risk);

    let filter = device_query::DeviceProfileFilter {
        application_id: &query.application_id,
        environment_id: query.environment_id.as_deref(),
        last_seen_from,
        last_seen_to,
        min_risk: risk_min,
        max_risk: risk_max,
        search: query.search.as_deref(),
        page: query.page.max(1),
        page_size: query.page_size.clamp(1, 200),
    };
    let page = device_query::list_profiles(&installed.database, &filter).await?;
    let summary = device_query::security_summary(
        &installed.database,
        &query.application_id,
        query.environment_id.as_deref(),
        active_since,
        recent_since,
    )
    .await?;

    let page_number = filter.page;
    let page_size = filter.page_size;
    let total = page.total;
    let has_more = page_number.saturating_mul(page_size) < total;
    Ok(DevicePage {
        items: page
            .items
            .into_iter()
            .map(|record| map_device(record, active_since, recent_since))
            .collect(),
        page: page_number,
        page_size,
        total,
        has_more,
        summary: DeviceSecuritySummary {
            total: summary.total,
            active: summary.active,
            recent: summary.recent,
            offline: summary.offline,
            high_risk: summary.high_risk,
            critical: summary.critical,
        },
    })
}

fn status_window(
    status: Option<DeviceStatusFilter>,
    active_since: i64,
    recent_since: i64,
) -> (Option<i64>, Option<i64>) {
    match status {
        Some(DeviceStatusFilter::Active) => (Some(active_since), None),
        Some(DeviceStatusFilter::Recent) => (Some(recent_since), Some(active_since)),
        Some(DeviceStatusFilter::Offline) => (None, Some(recent_since)),
        None => (None, None),
    }
}

fn risk_window(
    risk: Option<DeviceRiskFilter>,
    explicit_min: Option<i32>,
) -> (Option<i32>, Option<i32>) {
    let (filter_min, max) = match risk {
        Some(DeviceRiskFilter::Low) => (Some(0), Some(19)),
        Some(DeviceRiskFilter::Medium) => (Some(20), Some(49)),
        Some(DeviceRiskFilter::High) => (Some(50), Some(79)),
        Some(DeviceRiskFilter::Critical) => (Some(80), Some(100)),
        Some(DeviceRiskFilter::Risky) => (Some(20), Some(100)),
        None => (None, None),
    };
    let min = match (filter_min, explicit_min) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (left, right) => left.or(right),
    };
    (min, max)
}

fn map_device(
    record: device_query::DeviceProfileRecord,
    active_since: i64,
    recent_since: i64,
) -> DeviceSummary {
    DeviceSummary {
        id: record.id,
        environment_id: record.environment_id,
        status: classify_status(record.last_seen_at, active_since, recent_since),
        risk_score: record.risk_score,
        risk_level: classify_risk(record.risk_score),
        last_seen_at: record.last_seen_at,
        last_event_at: record.last_event_at,
        last_metric_at: record.last_metric_at,
        last_log_at: record.last_log_at,
        last_error_at: record.last_error_at,
        session_id: record.last_session_id,
        app_version: record.last_app_version,
        launcher_version: record.last_launcher_version,
        os: record.last_os,
        event_items: record.event_items,
        metric_items: record.metric_items,
        log_items: record.log_items,
        error_items: record.error_items,
        session_changes: record.session_changes,
        app_version_changes: record.app_version_changes,
        launcher_version_changes: record.launcher_version_changes,
        os_changes: record.os_changes,
        anomaly_reasons: record
            .last_anomaly
            .as_deref()
            .map(|value| {
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        last_anomaly_at: record.last_anomaly_at,
    }
}

fn classify_status(last_seen_at: i64, active_since: i64, recent_since: i64) -> DeviceStatus {
    if last_seen_at >= active_since {
        DeviceStatus::Active
    } else if last_seen_at >= recent_since {
        DeviceStatus::Recent
    } else {
        DeviceStatus::Offline
    }
}

fn classify_risk(score: i32) -> DeviceRiskLevel {
    match score {
        80.. => DeviceRiskLevel::Critical,
        50..=79 => DeviceRiskLevel::High,
        20..=49 => DeviceRiskLevel::Medium,
        _ => DeviceRiskLevel::Low,
    }
}

#[cfg(test)]
mod tests {
    use super::{classify_risk, classify_status, DeviceRiskLevel, DeviceStatus};

    #[test]
    fn status_thresholds_are_stable() {
        assert!(matches!(classify_status(100, 90, 10), DeviceStatus::Active));
        assert!(matches!(classify_status(50, 90, 10), DeviceStatus::Recent));
        assert!(matches!(classify_status(5, 90, 10), DeviceStatus::Offline));
    }

    #[test]
    fn risk_thresholds_are_stable() {
        assert!(matches!(classify_risk(0), DeviceRiskLevel::Low));
        assert!(matches!(classify_risk(20), DeviceRiskLevel::Medium));
        assert!(matches!(classify_risk(50), DeviceRiskLevel::High));
        assert!(matches!(classify_risk(80), DeviceRiskLevel::Critical));
    }
}
