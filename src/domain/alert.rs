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
        Ok(())
    }
}
