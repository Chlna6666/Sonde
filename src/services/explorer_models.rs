use serde::Serialize;
use serde_json::value::RawValue;

#[derive(Clone, Debug)]
pub struct ExplorerFilter {
    pub application_id: String,
    pub environment_id: Option<String>,
    pub from: Option<i64>,
    pub to: Option<i64>,
    pub name: Option<String>,
    pub level: Option<String>,
    pub text: Option<String>,
    pub page: u64,
    pub page_size: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    pub items: Vec<T>,
    pub page: u64,
    pub page_size: u64,
    pub has_more: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventRecord {
    pub id: String,
    pub name: String,
    pub timestamp: i64,
    pub anonymous_id: Option<String>,
    pub app_version: Option<String>,
    pub launcher_version: Option<String>,
    pub os: Option<String>,
    pub attributes: Box<RawValue>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricRecord {
    pub id: String,
    pub name: String,
    pub metric_type: String,
    /// Compatibility projection retained for existing Explorer consumers. Histogram-aware clients
    /// should read `histogram` so population weighting is not lost.
    pub value: f64,
    pub histogram: Option<HistogramRecord>,
    pub unit: Option<String>,
    pub timestamp: i64,
    pub attributes: Box<RawValue>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistogramRecord {
    pub count: u64,
    pub sum: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub explicit_bounds: Vec<f64>,
    pub bucket_counts: Vec<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogRecord {
    pub id: String,
    pub level: String,
    pub message: String,
    pub logger: Option<String>,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub timestamp: i64,
    pub attributes: Box<RawValue>,
}
