mod client;
mod delivery;
mod device_id;
mod error;
mod model;
mod signing;
mod spool;
#[cfg(test)]
mod spool_tests;

pub use client::{SondeClient, SondeClientBuilder};
pub use delivery::{DeliveryOptions, DeliveryStats, QueueDeliveryStats, RetryPolicy};
pub use device_id::{generate_device_id, load_or_create_device_id};
pub use error::{Error, Result};
pub use model::{
    Attributes, DeviceFacts, ErrorEvent, ErrorSeverity, Event, Histogram, LogEntry, LogLevel, Metric,
    MetricType,
};
pub use spool::SpoolOptions;
