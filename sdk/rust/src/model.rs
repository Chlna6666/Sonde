use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type Attributes = BTreeMap<String, Value>;

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceFacts {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launcher_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub architecture: Option<String>,
}

impl DeviceFacts {
    pub(crate) fn with_platform_defaults() -> Self {
        Self {
            os: Some(std::env::consts::OS.to_owned()),
            architecture: Some(std::env::consts::ARCH.to_owned()),
            ..Self::default()
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.app_version.is_none()
            && self.launcher_version.is_none()
            && self.os.is_none()
            && self.system_language.is_none()
            && self.architecture.is_none()
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: Attributes,
}

impl Event {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            idempotency_key: None,
            attributes: Attributes::new(),
        }
    }

    pub fn attribute(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }

    pub fn idempotency_key(mut self, key: impl Into<String>) -> Self {
        self.idempotency_key = Some(key.into());
        self
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MetricType {
    Counter,
    Gauge,
    Histogram,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Histogram {
    pub count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sum: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub explicit_bounds: Vec<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bucket_counts: Vec<u64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metric {
    pub name: String,
    pub metric_type: MetricType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub histogram: Option<Histogram>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: Attributes,
}

impl Metric {
    pub fn gauge(name: impl Into<String>, value: f64) -> Self {
        Self::scalar(name, MetricType::Gauge, value)
    }

    pub fn counter(name: impl Into<String>, value: f64) -> Self {
        Self::scalar(name, MetricType::Counter, value)
    }

    pub fn histogram(name: impl Into<String>, histogram: Histogram) -> Self {
        Self {
            name: name.into(),
            metric_type: MetricType::Histogram,
            value: None,
            histogram: Some(histogram),
            unit: None,
            attributes: Attributes::new(),
        }
    }

    fn scalar(name: impl Into<String>, metric_type: MetricType, value: f64) -> Self {
        Self {
            name: name.into(),
            metric_type,
            value: Some(value),
            histogram: None,
            unit: None,
            attributes: Attributes::new(),
        }
    }

    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    pub fn attribute(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub level: LogLevel,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logger: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span_id: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: Attributes,
}

impl LogEntry {
    pub fn new(level: LogLevel, message: impl Into<String>) -> Self {
        Self {
            level,
            message: message.into(),
            logger: None,
            trace_id: None,
            span_id: None,
            attributes: Attributes::new(),
        }
    }

    pub fn logger(mut self, logger: impl Into<String>) -> Self {
        self.logger = Some(logger.into());
        self
    }

    pub fn attribute(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ErrorSeverity {
    Fatal,
    Error,
    Warning,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorEvent {
    pub name: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stack_trace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<ErrorSeverity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handled: Option<bool>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: Attributes,
}

impl ErrorEvent {
    pub fn new(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            message: message.into(),
            stack_trace: None,
            severity: None,
            handled: None,
            attributes: Attributes::new(),
        }
    }

    pub fn stack_trace(mut self, stack_trace: impl Into<String>) -> Self {
        self.stack_trace = Some(stack_trace.into());
        self
    }

    pub fn severity(mut self, severity: ErrorSeverity) -> Self {
        self.severity = Some(severity);
        self
    }

    pub fn handled(mut self, handled: bool) -> Self {
        self.handled = Some(handled);
        self
    }

    pub fn attribute(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchReceipt {
    pub accepted: usize,
    pub rejected: Vec<RejectedItem>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RejectedItem {
    pub index: usize,
    pub reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TokenResponse {
    pub token: String,
    pub expires_at: i64,
    pub signing_key: String,
    pub signature_version: String,
}

#[derive(Serialize)]
pub(crate) struct TokenRequest<'a> {
    #[serde(rename = "deviceId")]
    pub device_id: &'a str,
}

#[derive(Serialize)]
pub(crate) struct BatchRef<'a, T> {
    pub items: &'a [T],
}
