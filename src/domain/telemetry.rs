use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_BATCH_ITEMS: usize = 1_000;

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

impl ValidateTelemetry for ErrorInput {
    fn validate(&self) -> Result<(), &'static str> {
        validate_name(&self.name)?;
        if self.message.is_empty() || self.message.len() > 16_384 {
            return Err("error message must be 1..16384 bytes");
        }
        if let Some(ref st) = self.stack_trace {
            if st.len() > 65_536 {
                return Err("stack_trace must be at most 65536 bytes");
            }
        }
        validate_attributes(&self.attributes)
    }
}

pub trait ValidateTelemetry {
    fn validate(&self) -> Result<(), &'static str>;
}

impl ValidateTelemetry for EventInput {
    fn validate(&self) -> Result<(), &'static str> {
        validate_name(&self.name)?;
        validate_attributes(&self.attributes)
    }
}

impl ValidateTelemetry for MetricInput {
    fn validate(&self) -> Result<(), &'static str> {
        validate_name(&self.name)?;
        if !self.value.is_finite() {
            return Err("metric value must be finite");
        }
        validate_attributes(&self.attributes)
    }
}

impl ValidateTelemetry for LogInput {
    fn validate(&self) -> Result<(), &'static str> {
        if self.message.is_empty() || self.message.len() > 16_384 {
            return Err("message must be 1..16384 bytes");
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
    Ok(())
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
}
