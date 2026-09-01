mod client;
mod error;
mod model;
mod signing;

pub use client::{SondeClient, SondeClientBuilder};
pub use error::{Error, Result};
pub use model::{
    Attributes, BatchReceipt, DeviceFacts, ErrorEvent, ErrorSeverity, Event, Histogram, LogEntry,
    LogLevel, Metric, MetricType, RejectedItem,
};

/// Generate a high-entropy pseudonymous installation/device identifier.
///
/// Persist the returned value on first launch and reuse it for the lifetime of the installation.
/// It is intentionally unrelated to hardware serials, account names, or other direct identifiers.
pub fn generate_device_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
