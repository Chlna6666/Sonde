use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_BATCH_ITEMS: usize = 1_000;
const MAX_FUTURE_SKEW_MILLIS: i64 = 5 * 60 * 1_000;
const MAX_ATTRIBUTE_DEPTH: usize = 6;
const MAX_ATTRIBUTE_STRING_BYTES: usize = 16_384;
const MAX_ATTRIBUTE_ARRAY_ITEMS: usize = 128;
const MAX_ATTRIBUTE_OBJECT_ITEMS: usize = 64;

pub type Attributes = BTreeMap<String, Value>;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventInput {
    pub name: String,
    pub timestamp: Option<i64>,
    pub anonymous_id: Option<String>,
    pub session_id: Option<String>,
    pub app_version: Option<String>,
    pub launcher_version: Option<String>,
    pub os: Option<String>,
    #[serde(default)]
    pub attributes: Attributes,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricInput {
    pub name: String,
    pub metric_type: MetricType,
    pub value: f64,
    pub unit: Option<String>,
    pub timestamp: Option<i64>,
    #[serde(default)]
    pub attributes: Attributes,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MetricType {
    Counter,
    Gauge,
    Histogram,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogInput {
    pub level: LogLevel,
    pub message: String,
    pub logger: Option<String>,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub timestamp: Option<i64>,
    #[serde(default)]
    pub attributes: Attributes,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Batch<T> {
    pub items: Vec<T>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchReceipt {
    pub accepted: usize,
    pub rejected: Vec<RejectedItem>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RejectedItem {
    pub index: usize,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ErrorSeverity {
    Fatal,
    Error,
    Warning,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorInput {
    pub name: String,
    pub message: String,
    pub stack_trace: Option<String>,
    pub severity: Option<ErrorSeverity>,
    pub handled: Option<bool>,
    pub timestamp: Option<i64>,
    pub anonymous_id: Option<String>,
    pub session_id: Option<String>,
    pub app_version: Option<String>,
    pub launcher_version: Option<String>,
    pub os: Option<String>,
    #[serde(default)]
    pub attributes: Attributes,
}

pub trait ValidateTelemetry {
    fn validate(&self) -> Result<(), &'static str>;
}

impl ValidateTelemetry for ErrorInput {
    fn validate(&self) -> Result<(), &'static str> {
        validate_name(&self.name)?;
        validate_timestamp(self.timestamp)?;
        validate_common_dimensions(
            self.anonymous_id.as_deref(),
            self.session_id.as_deref(),
            self.app_version.as_deref(),
            self.launcher_version.as_deref(),
            self.os.as_deref(),
        )?;
        if self.message.is_empty() || self.message.len() > 16_384 {
            return Err("error message must be 1..16384 bytes");
        }
        if let Some(stack_trace) = &self.stack_trace
            && stack_trace.len() > 65_536
        {
            return Err("stack_trace must be at most 65536 bytes");
        }
        validate_attributes(&self.attributes)
    }
}

impl ValidateTelemetry for EventInput {
    fn validate(&self) -> Result<(), &'static str> {
        validate_name(&self.name)?;
        validate_timestamp(self.timestamp)?;
        validate_common_dimensions(
            self.anonymous_id.as_deref(),
            self.session_id.as_deref(),
            self.app_version.as_deref(),
            self.launcher_version.as_deref(),
            self.os.as_deref(),
        )?;
        validate_attributes(&self.attributes)
    }
}

impl ValidateTelemetry for MetricInput {
    fn validate(&self) -> Result<(), &'static str> {
        validate_name(&self.name)?;
        validate_timestamp(self.timestamp)?;
        if !self.value.is_finite() {
            return Err("metric value must be finite");
        }
        if self.unit.as_ref().is_some_and(|unit| unit.len() > 64) {
            return Err("metric unit must be at most 64 bytes");
        }
        validate_attributes(&self.attributes)
    }
}

impl ValidateTelemetry for LogInput {
    fn validate(&self) -> Result<(), &'static str> {
        validate_timestamp(self.timestamp)?;
        if self.message.is_empty() || self.message.len() > 16_384 {
            return Err("message must be 1..16384 bytes");
        }
        if self.logger.as_ref().is_some_and(|logger| logger.len() > 256) {
            return Err("logger must be at most 256 bytes");
        }
        if self.trace_id.as_ref().is_some_and(|value| value.len() > 64)
            || self.span_id.as_ref().is_some_and(|value| value.len() > 64)
        {
            return Err("trace_id and span_id must be at most 64 bytes");
        }
        validate_attributes(&self.attributes)
    }
}

fn validate_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() || name.len() > 128 {
        return Err("name must be 1..128 bytes");
    }
    Ok(())
}

fn validate_timestamp(timestamp: Option<i64>) -> Result<(), &'static str> {
    if let Some(timestamp) = timestamp {
        let max_timestamp = chrono::Utc::now()
            .timestamp_millis()
            .saturating_add(MAX_FUTURE_SKEW_MILLIS);
        if timestamp > max_timestamp {
            return Err("timestamp must not be more than 5 minutes in the future");
        }
    }
    Ok(())
}

fn validate_common_dimensions(
    anonymous_id: Option<&str>,
    session_id: Option<&str>,
    app_version: Option<&str>,
    launcher_version: Option<&str>,
    os: Option<&str>,
) -> Result<(), &'static str> {
    if anonymous_id.is_some_and(|value| value.is_empty() || value.len() > 512) {
        return Err("anonymous_id must be 1..512 bytes when provided");
    }
    if session_id.is_some_and(|value| value.is_empty() || value.len() > 256) {
        return Err("session_id must be 1..256 bytes when provided");
    }
    if app_version.is_some_and(|value| value.len() > 128)
        || launcher_version.is_some_and(|value| value.len() > 128)
    {
        return Err("version fields must be at most 128 bytes");
    }
    if os.is_some_and(|value| value.len() > 256) {
        return Err("os must be at most 256 bytes");
    }
    Ok(())
}

fn validate_attributes(attributes: &Attributes) -> Result<(), &'static str> {
    if attributes.len() > 64 {
        return Err("at most 64 attributes are allowed");
    }
    if attributes
        .keys()
        .any(|key| key.is_empty() || key.len() > 64)
    {
        return Err("attribute keys must be 1..64 bytes");
    }
    if attributes
        .values()
        .any(|value| !valid_attribute_value(value, 0))
    {
        return Err("attribute values exceed the allowed size or nesting limits");
    }
    Ok(())
}

fn valid_attribute_value(value: &Value, depth: usize) -> bool {
    if depth > MAX_ATTRIBUTE_DEPTH {
        return false;
    }
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => true,
        Value::String(value) => value.len() <= MAX_ATTRIBUTE_STRING_BYTES,
        Value::Array(values) => {
            values.len() <= MAX_ATTRIBUTE_ARRAY_ITEMS
                && values
                    .iter()
                    .all(|value| valid_attribute_value(value, depth + 1))
        }
        Value::Object(values) => {
            values.len() <= MAX_ATTRIBUTE_OBJECT_ITEMS
                && values.iter().all(|(key, value)| {
                    !key.is_empty()
                        && key.len() <= 64
                        && valid_attribute_value(value, depth + 1)
                })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Attributes, EventInput, ValidateTelemetry};

    #[test]
    fn empty_event_name_is_rejected() {
        let event = EventInput {
            name: String::new(),
            timestamp: None,
            anonymous_id: None,
            session_id: None,
            app_version: None,
            launcher_version: None,
            os: None,
            attributes: Attributes::new(),
        };
        assert!(event.validate().is_err());
    }

    #[test]
    fn far_future_timestamp_is_rejected() {
        let event = EventInput {
            name: "application.start".into(),
            timestamp: Some(chrono::Utc::now().timestamp_millis() + 10 * 60 * 1_000),
            anonymous_id: None,
            session_id: None,
            app_version: None,
            launcher_version: None,
            os: None,
            attributes: Attributes::new(),
        };
        assert!(event.validate().is_err());
    }
}
