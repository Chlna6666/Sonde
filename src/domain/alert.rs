use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertExpression {
    pub source: AlertSource,
    pub operator: Comparison,
    pub threshold: f64,
    pub window_minutes: u16,
    pub consecutive_hits: u16,
    pub filters: Vec<AlertFilter>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertSource {
    EventCount,
    MetricAverage,
    MetricSum,
    LogCount,
    MissingData,
    ChangeRate,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Comparison {
    GreaterThan,
    GreaterOrEqual,
    LessThan,
    LessOrEqual,
    Equal,
}

impl Comparison {
    pub fn evaluate(&self, value: f64, threshold: f64) -> bool {
        match self {
            Comparison::GreaterThan => value > threshold,
            Comparison::GreaterOrEqual => value >= threshold,
            Comparison::LessThan => value < threshold,
            Comparison::LessOrEqual => value <= threshold,
            Comparison::Equal => (value - threshold).abs() < 1e-6,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AlertFilter {
    pub field: String,
    pub value: String,
}

impl AlertExpression {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !matches!(self.window_minutes, 1 | 5 | 15 | 60) {
            return Err("window must be 1, 5, 15, or 60 minutes");
        }
        if self.consecutive_hits == 0 || self.consecutive_hits > 100 {
            return Err("consecutive hits must be 1..100");
        }
        if !self.threshold.is_finite() {
            return Err("threshold must be finite");
        }
        if self.filters.len() > 8 {
            return Err("at most 8 alert filters are allowed");
        }
        for filter in &self.filters {
            if filter.value.is_empty() || filter.value.len() > 256 {
                return Err("alert filter values must be 1..256 bytes");
            }
            if !self.allowed_filter_fields().contains(&filter.field.as_str()) {
                return Err("alert filter field is not supported for this source");
            }
        }
        Ok(())
    }

    fn allowed_filter_fields(&self) -> &'static [&'static str] {
        match self.source {
            AlertSource::EventCount | AlertSource::MissingData | AlertSource::ChangeRate => &[
                "environment_id",
                "name",
                "app_version",
                "launcher_version",
                "os",
            ],
            AlertSource::MetricAverage | AlertSource::MetricSum => {
                &["environment_id", "name", "metric_type", "unit"]
            }
            AlertSource::LogCount => &[
                "environment_id",
                "level",
                "logger",
                "trace_id",
                "span_id",
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AlertExpression, AlertFilter, AlertSource, Comparison};

    fn expression(source: AlertSource, field: &str) -> AlertExpression {
        AlertExpression {
            source,
            operator: Comparison::GreaterOrEqual,
            threshold: 1.0,
            window_minutes: 5,
            consecutive_hits: 1,
            filters: vec![AlertFilter {
                field: field.into(),
                value: "value".into(),
            }],
        }
    }

    #[test]
    fn rejects_unknown_filter_columns() {
        assert!(expression(AlertSource::EventCount, "password_hash").validate().is_err());
        assert!(expression(AlertSource::MetricAverage, "message").validate().is_err());
    }

    #[test]
    fn accepts_supported_filter_columns() {
        assert!(expression(AlertSource::EventCount, "app_version").validate().is_ok());
        assert!(expression(AlertSource::LogCount, "level").validate().is_ok());
    }
}
