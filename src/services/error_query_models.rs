use serde::Serialize;

#[derive(Clone, Debug)]
pub struct ErrorGroupFilter {
    pub application_id: String,
    pub environment_id: Option<String>,
    pub severity: Option<String>,
    pub from: Option<i64>,
    pub to: Option<i64>,
    pub page: u64,
    pub page_size: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorGroupRecord {
    pub id: String,
    pub application_id: String,
    pub environment_id: String,
    pub fingerprint: String,
    pub name: String,
    pub message_sample: String,
    pub severity: String,
    pub first_seen: i64,
    pub last_seen: i64,
    pub occurrences: u64,
    pub last_app_version: Option<String>,
    pub last_launcher_version: Option<String>,
    pub last_os: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorGroupPage {
    pub items: Vec<ErrorGroupRecord>,
    pub page: u64,
    pub page_size: u64,
    pub has_more: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorOccurrenceRecord {
    pub id: String,
    pub group_id: String,
    pub timestamp: i64,
    pub anonymous_id: Option<String>,
    pub session_id: Option<String>,
    pub app_version: Option<String>,
    pub launcher_version: Option<String>,
    pub os: Option<String>,
    pub stack_trace: Option<String>,
    pub handled: Option<bool>,
    pub attributes: Box<serde_json::value::RawValue>,
    pub received_at: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorOccurrencePage {
    pub items: Vec<ErrorOccurrenceRecord>,
    pub page: u64,
    pub page_size: u64,
    pub has_more: bool,
}
