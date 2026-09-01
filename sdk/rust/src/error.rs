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
    #[error("failed to serialize telemetry payload: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Sonde returned HTTP {status}: {body}")]
    Api { status: StatusCode, body: String },
    #[error("failed to initialize HMAC request signer")]
    SigningKey,
}

pub type Result<T> = std::result::Result<T, Error>;
