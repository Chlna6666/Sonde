#[derive(Clone, Debug)]
pub struct DeviceQuery {
    pub application_id: String,
    pub environment_id: Option<String>,
    pub status: Option<DeviceStatusFilter>,
    pub risk: Option<DeviceRiskFilter>,
    pub min_risk: Option<i32>,
    pub search: Option<String>,
    pub page: u64,
    pub page_size: u64,
}

#[derive(Clone, Copy, Debug)]
pub enum DeviceStatusFilter {
    Active,
    Recent,
    Offline,
}

#[derive(Clone, Copy, Debug)]
pub enum DeviceRiskFilter {
    Low,
    Medium,
    High,
    Critical,
    Risky,
}

#[derive(Clone, Copy, Debug, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceStatus {
    Active,
    Recent,
    Offline,
}

#[derive(Clone, Copy, Debug, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceRiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSummary {
    pub id: String,
    pub environment_id: String,
    pub status: DeviceStatus,
    pub risk_score: i32,
    pub risk_level: DeviceRiskLevel,
    pub last_seen_at: i64,
    pub last_event_at: Option<i64>,
    pub last_metric_at: Option<i64>,
    pub last_log_at: Option<i64>,
    pub last_error_at: Option<i64>,
    pub session_id: Option<String>,
    pub app_version: Option<String>,
    pub launcher_version: Option<String>,
    pub os: Option<String>,
    pub system_language: Option<String>,
    pub architecture: Option<String>,
    pub event_items: i64,
    pub metric_items: i64,
    pub log_items: i64,
    pub error_items: i64,
    pub session_changes: i64,
    pub app_version_changes: i64,
    pub launcher_version_changes: i64,
    pub os_changes: i64,
    pub anomaly_reasons: Vec<String>,
    pub last_anomaly_at: Option<i64>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSecuritySummary {
    pub total: u64,
    pub active: u64,
    pub recent: u64,
    pub offline: u64,
    pub high_risk: u64,
    pub critical: u64,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicePage {
    pub items: Vec<DeviceSummary>,
    pub page: u64,
    pub page_size: u64,
    pub total: u64,
    pub has_more: bool,
    pub summary: DeviceSecuritySummary,
}
