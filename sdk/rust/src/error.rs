use std::{io, path::PathBuf, time::Duration};

use reqwest::StatusCode;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid SDK configuration: {0}")]
    InvalidConfiguration(String),
    #[error("system clock is before the Unix epoch or exceeds the supported range")]
    InvalidClock,
    #[error("telemetry batch must contain between 1 and 1000 items")]
    InvalidBatchSize,
    #[error("serialized telemetry payload exceeds Sonde's 1 MiB ingest limit")]
    PayloadTooLarge,
    #[error("unsupported Sonde request signature version: {0}")]
    UnsupportedSignatureVersion(String),
    #[error("telemetry {kind} queue is full")]
    QueueFull { kind: &'static str },
    #[error("telemetry {kind} queue is closed")]
    QueueClosed { kind: &'static str },
    #[error("non-blocking enqueue is unavailable for durable {kind} delivery; use the async enqueue API")]
    DurableEnqueueRequiresAsync { kind: &'static str },
    #[error("telemetry {kind} spool reached its {max_bytes} byte capacity")]
    SpoolFull {
        kind: &'static str,
        max_bytes: u64,
    },
    #[error("telemetry spool at {path:?} is already locked by another client/process")]
    SpoolLocked { path: PathBuf },
    #[error("telemetry spool at {path:?} belongs to a different Sonde endpoint/device/bootstrap key")]
    SpoolBindingMismatch { path: PathBuf },
    #[error("failed to access telemetry spool at {path:?}: {source}")]
    SpoolIo {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("telemetry spool at {path:?} is corrupt: {reason}")]
    SpoolCorrupt { path: PathBuf, reason: String },
    #[error("Sonde client is shutting down")]
    ShuttingDown,
    #[error("Sonde rejected {rejected} item(s) from the {kind} delivery queue")]
    RejectedItems {
        kind: &'static str,
        rejected: usize,
    },
    #[error("Sonde returned an invalid or ambiguous delivery response: {0}")]
    InvalidServerResponse(String),
    #[error("failed to access Sonde device ID storage at {path:?}: {source}")]
    DeviceIdStorage {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("stored Sonde device ID at {path:?} is invalid: {reason}")]
    InvalidStoredDeviceId {
        path: PathBuf,
        reason: &'static str,
    },
    #[error("failed to serialize telemetry payload: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Sonde returned HTTP {status}: {body}")]
    Api {
        status: StatusCode,
        body: String,
        retry_after: Option<Duration>,
    },
    #[error("failed to initialize HMAC request signer")]
    SigningKey,
}

impl Error {
    pub(crate) fn is_retryable(&self) -> bool {
        match self {
            Self::Http(error) => error.is_connect() || error.is_timeout(),
            Self::Api { status, .. } => {
                *status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
            }
            Self::InvalidServerResponse(_) => true,
            _ => false,
        }
    }

    pub(crate) fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::Api { retry_after, .. } => *retry_after,
            _ => None,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
