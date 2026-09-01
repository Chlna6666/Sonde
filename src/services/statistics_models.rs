use serde::Serialize;

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
pub struct ActivitySummary {
    pub active_millis: u64,
    pub lifetime_active_millis: u64,
    pub sessions: u64,
    pub lifetime_sessions: u64,
    pub measured_devices: u64,
    pub measurement_coverage_pct: f64,
    pub average_session_millis: u64,
    pub average_active_millis_per_device: u64,
    pub stickiness_pct: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityTrendPoint {
    pub bucket: String,
    pub active_users: u64,
    pub active_millis: u64,
    pub sessions: u64,
    pub average_session_millis: u64,
    pub cumulative_active_millis: u64,
    pub cumulative_sessions: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityStats {
    pub summary: ActivitySummary,
    pub trend: Vec<ActivityTrendPoint>,
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
    pub activity: ActivityStats,
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
    pub activity: ActivityStats,
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
