use serde::{Deserialize, Serialize};

pub use super::attributes::{AttributeMap, Attributes};

pub const MAX_BATCH_ITEMS: usize = 1_000;
const MAX_FUTURE_SKEW_MILLIS: i64 = 5 * 60 * 1_000;
const MAX_PAST_AGE_MILLIS: i64 = 7 * 24 * 60 * 60 * 1_000;
const MAX_ABS_METRIC_VALUE: f64 = 1.0e18;
const MAX_HISTOGRAM_COUNT: u64 = 10_000_000;
const MAX_HISTOGRAM_BOUNDS: usize = 256;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
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
    pub system_language: Option<String>,
    #[serde(default)]
    pub architecture: Option<String>,
    /// Stable client-generated key used to make event retries idempotent within one app/environment.
    pub idempotency_key: Option<String>,
    #[serde(default)]
    pub attributes: Attributes,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricInput {
    pub name: String,
    pub metric_type: MetricType,
    /// Scalar value for counters/gauges and the legacy one-observation histogram representation.
    /// Aggregated histograms use `histogram` and may omit this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub histogram: Option<HistogramInput>,
    pub unit: Option<String>,
    pub timestamp: Option<i64>,
    #[serde(default)]
    pub attributes: Attributes,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistogramInput {
    pub count: u64,
    pub sum: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    #[serde(default)]
    pub explicit_bounds: Vec<f64>,
    #[serde(default)]
    pub bucket_counts: Vec<u64>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MetricType {
    Counter,
    Gauge,
    Histogram,
}

impl MetricType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Counter => "counter",
            Self::Gauge => "gauge",
            Self::Histogram => "histogram",
        }
    }
}

impl MetricInput {
    /// Returns the scalar projection retained for compatibility with existing metric queries.
    /// Histogram-aware callers should use the stored histogram population instead.
    pub fn compatibility_value(&self) -> f64 {
        if let Some(value) = self.value {
            return value;
        }
        let Some(histogram) = self.histogram.as_ref() else {
            return 0.0;
        };
        if histogram.count > 0
            && let Some(sum) = histogram.sum
        {
            return sum / histogram.count as f64;
        }
        if let (Some(min), Some(max)) = (histogram.min, histogram.max)
            && min == max
        {
            return min;
        }
        0.0
    }

    /// Normalizes the legacy `histogram + value` representation to the aggregate data model.
    pub fn normalized_histogram(&self) -> Option<HistogramInput> {
        if self.metric_type != MetricType::Histogram {
            return None;
        }
        self.histogram.clone().or_else(|| {
            self.value.map(|value| HistogramInput {
                count: 1,
                sum: Some(value),
                min: Some(value),
                max: Some(value),
                explicit_bounds: Vec::new(),
                // An explicit histogram with no finite bounds still has one +Inf bucket.
                bucket_counts: vec![1],
            })
        })
    }
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

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
}

impl LogLevel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
            Self::Fatal => "fatal",
        }
    }
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
    pub reason: &'static str,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ErrorSeverity {
    Fatal,
    Error,
    Warning,
}

impl ErrorSeverity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fatal => "fatal",
            Self::Error => "error",
            Self::Warning => "warning",
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
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
    pub system_language: Option<String>,
    #[serde(default)]
    pub architecture: Option<String>,
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
            self.system_language.as_deref(),
            self.architecture.as_deref(),
        )?;
        if self.message.is_empty() || self.message.len() > 16_384 {
            return Err("error message must be 1..16384 bytes");
        }
        if let Some(stack_trace) = &self.stack_trace
            && stack_trace.len() > 65_536
        {
            return Err("stack_trace must be at most 65536 bytes");
        }
        self.attributes.validate()
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
            self.system_language.as_deref(),
            self.architecture.as_deref(),
        )?;
        if self
            .idempotency_key
            .as_ref()
            .is_some_and(|key| key.is_empty() || key.len() > 128)
        {
            return Err("idempotency_key must be 1..128 bytes when provided");
        }
        self.attributes.validate()
    }
}

impl ValidateTelemetry for MetricInput {
    fn validate(&self) -> Result<(), &'static str> {
        validate_name(&self.name)?;
        validate_timestamp(self.timestamp)?;
        if self.unit.as_ref().is_some_and(|unit| unit.len() > 64) {
            return Err("metric unit must be at most 64 bytes");
        }
        if self
            .value
            .is_some_and(|value| !value.is_finite() || value.abs() > MAX_ABS_METRIC_VALUE)
        {
            return Err("metric value must be finite and within +/-1e18");
        }

        match self.metric_type {
            MetricType::Counter | MetricType::Gauge => {
                if self.histogram.is_some() {
                    return Err("counter and gauge metrics must not include histogram data");
                }
                if self.value.is_none() {
                    return Err("counter and gauge metrics require value");
                }
            }
            MetricType::Histogram => match (&self.value, &self.histogram) {
                (None, None) => return Err("histogram metric requires value or histogram data"),
                (Some(_), Some(_)) => {
                    return Err("histogram metric must use either value or histogram data");
                }
                (None, Some(histogram)) => validate_histogram(histogram)?,
                (Some(_), None) => {}
            },
        }
        self.attributes.validate()
    }
}

impl ValidateTelemetry for LogInput {
    fn validate(&self) -> Result<(), &'static str> {
        validate_timestamp(self.timestamp)?;
        if self.message.is_empty() || self.message.len() > 16_384 {
            return Err("message must be 1..16384 bytes");
        }
        if self
            .logger
            .as_ref()
            .is_some_and(|logger| logger.len() > 256)
        {
            return Err("logger must be at most 256 bytes");
        }
        if self.trace_id.as_ref().is_some_and(|value| value.len() > 64)
            || self.span_id.as_ref().is_some_and(|value| value.len() > 64)
        {
            return Err("trace_id and span_id must be at most 64 bytes");
        }
        self.attributes.validate()
    }
}

fn validate_histogram(histogram: &HistogramInput) -> Result<(), &'static str> {
    if histogram.count > MAX_HISTOGRAM_COUNT {
        return Err("histogram count must not exceed 10000000 per point");
    }
    if histogram.explicit_bounds.len() > MAX_HISTOGRAM_BOUNDS {
        return Err("histogram supports at most 256 explicit bounds");
    }
    if histogram
        .sum
        .into_iter()
        .chain(histogram.min)
        .chain(histogram.max)
        .any(|value| !value.is_finite() || value.abs() > MAX_ABS_METRIC_VALUE)
    {
        return Err("histogram sum/min/max must be finite and within +/-1e18");
    }
    if histogram
        .explicit_bounds
        .iter()
        .any(|value| !value.is_finite() || value.abs() > MAX_ABS_METRIC_VALUE)
    {
        return Err("histogram bounds must be finite and within +/-1e18");
    }
    if histogram
        .explicit_bounds
        .windows(2)
        .any(|pair| pair[0] >= pair[1])
    {
        return Err("histogram bounds must be strictly increasing");
    }
    if histogram.bucket_counts.len() != histogram.explicit_bounds.len().saturating_add(1) {
        return Err("histogram bucket count length must equal bounds length plus one");
    }
    let bucket_total = histogram
        .bucket_counts
        .iter()
        .try_fold(0_u64, |total, count| total.checked_add(*count))
        .ok_or("histogram bucket counts overflow")?;
    if bucket_total != histogram.count {
        return Err("histogram bucket counts must sum to count");
    }
    if let (Some(min), Some(max)) = (histogram.min, histogram.max)
        && min > max
    {
        return Err("histogram min must be less than or equal to max");
    }
    if histogram.count == 0 {
        if histogram.sum.is_some_and(|sum| sum != 0.0) {
            return Err("empty histogram sum must be zero when provided");
        }
        if histogram.min.is_some() || histogram.max.is_some() {
            return Err("empty histogram must not include min or max");
        }
    }
    Ok(())
}

fn validate_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() || name.len() > 128 {
        return Err("name must be 1..128 bytes");
    }
    Ok(())
}

fn validate_timestamp(timestamp: Option<i64>) -> Result<(), &'static str> {
    if let Some(timestamp) = timestamp {
        let now = chrono::Utc::now().timestamp_millis();
        if timestamp > now.saturating_add(MAX_FUTURE_SKEW_MILLIS) {
            return Err("timestamp must not be more than 5 minutes in the future");
        }
        if timestamp < now.saturating_sub(MAX_PAST_AGE_MILLIS) {
            return Err("timestamp must not be more than 7 days in the past for live ingestion");
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
    system_language: Option<&str>,
    architecture: Option<&str>,
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
    if system_language.is_some_and(|value| {
        value.is_empty() || value.len() > 64 || value.chars().any(|c| c.is_control())
    }) {
        return Err("systemLanguage must be at most 64 bytes");
    }
    if architecture.is_some_and(|value| {
        value.is_empty() || value.len() > 64 || value.chars().any(|c| c.is_control())
    }) {
        return Err("architecture must be at most 64 bytes");
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{
        Attributes, ErrorSeverity, EventInput, HistogramInput, LogLevel, MetricInput, MetricType,
        ValidateTelemetry,
    };

    fn event(name: &str) -> EventInput {
        EventInput {
            name: name.into(),
            timestamp: None,
            anonymous_id: None,
            session_id: None,
            app_version: None,
            launcher_version: None,
            os: None,
            system_language: None,
            architecture: None,
            idempotency_key: None,
            attributes: Attributes::new(),
        }
    }

    fn histogram() -> MetricInput {
        MetricInput {
            name: "http.request.duration".into(),
            metric_type: MetricType::Histogram,
            value: None,
            histogram: Some(HistogramInput {
                count: 4,
                sum: Some(40.0),
                min: Some(2.0),
                max: Some(20.0),
                explicit_bounds: vec![5.0, 10.0],
                bucket_counts: vec![1, 2, 1],
            }),
            unit: Some("ms".into()),
            timestamp: None,
            attributes: Attributes::new(),
        }
    }

    #[test]
    fn empty_event_name_is_rejected() {
        assert!(event("").validate().is_err());
    }

    #[test]
    fn far_future_timestamp_is_rejected() {
        let mut event = event("application.start");
        event.timestamp = Some(chrono::Utc::now().timestamp_millis() + 10 * 60 * 1_000);
        assert!(event.validate().is_err());
    }

    #[test]
    fn stale_live_timestamp_is_rejected() {
        let mut event = event("application.start");
        event.timestamp = Some(chrono::Utc::now().timestamp_millis() - 8 * 24 * 60 * 60 * 1_000);
        assert!(event.validate().is_err());
    }

    #[test]
    fn oversized_idempotency_key_is_rejected() {
        let mut event = event("application.start");
        event.idempotency_key = Some("x".repeat(129));
        assert!(event.validate().is_err());
    }

    #[test]
    fn absurd_metric_magnitude_is_rejected() {
        let metric = MetricInput {
            name: "counter".into(),
            metric_type: MetricType::Counter,
            value: Some(1.0e30),
            histogram: None,
            unit: None,
            timestamp: None,
            attributes: Attributes::new(),
        };
        assert!(metric.validate().is_err());
    }

    #[test]
    fn histogram_validates_otlp_bucket_invariants() {
        let metric = histogram();
        assert!(metric.validate().is_ok());
        assert_eq!(metric.compatibility_value(), 10.0);

        let mut invalid = histogram();
        invalid.histogram.as_mut().unwrap().bucket_counts = vec![1, 1, 1];
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn oversized_histogram_population_is_rejected() {
        let mut invalid = histogram();
        let histogram = invalid.histogram.as_mut().unwrap();
        histogram.count = 10_000_001;
        histogram.bucket_counts = vec![10_000_001, 0, 0];
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn histogram_without_finite_bounds_still_requires_inf_bucket() {
        let metric = MetricInput {
            name: "latency".into(),
            metric_type: MetricType::Histogram,
            value: None,
            histogram: Some(HistogramInput {
                count: 2,
                sum: Some(3.0),
                min: Some(1.0),
                max: Some(2.0),
                explicit_bounds: Vec::new(),
                bucket_counts: Vec::new(),
            }),
            unit: Some("ms".into()),
            timestamp: None,
            attributes: Attributes::new(),
        };
        assert!(metric.validate().is_err());
    }

    #[test]
    fn metric_and_log_labels_are_stable_lowercase() {
        assert_eq!(MetricType::Counter.as_str(), "counter");
        assert_eq!(MetricType::Histogram.as_str(), "histogram");
        assert_eq!(LogLevel::Error.as_str(), "error");
        assert_eq!(ErrorSeverity::Warning.as_str(), "warning");
    }

    #[test]
    fn legacy_histogram_value_normalizes_to_single_observation() {
        let metric = MetricInput {
            name: "latency".into(),
            metric_type: MetricType::Histogram,
            value: Some(12.5),
            histogram: None,
            unit: Some("ms".into()),
            timestamp: None,
            attributes: Attributes::new(),
        };
        assert!(metric.validate().is_ok());
        let histogram = metric.normalized_histogram().unwrap();
        assert_eq!(histogram.count, 1);
        assert_eq!(histogram.sum, Some(12.5));
        assert_eq!(histogram.min, Some(12.5));
        assert_eq!(histogram.max, Some(12.5));
        assert!(histogram.explicit_bounds.is_empty());
        assert_eq!(histogram.bucket_counts, vec![1]);
    }
}
