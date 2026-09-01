use std::{sync::Arc, time::{Duration, SystemTime, UNIX_EPOCH}};

use reqwest::{Client as HttpClient, Response, StatusCode, header::CONTENT_TYPE};
use serde::Serialize;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

use crate::{
    error::{Error, Result},
    model::{
        BatchReceipt, BatchRef, DeviceFacts, ErrorEvent, Event, LogEntry, Metric, TokenRequest,
        TokenResponse,
    },
    signing::{self, SIGNATURE_VERSION},
};

const MAX_BATCH_ITEMS: usize = 1_000;
const MAX_INGEST_BODY_BYTES: usize = 1_048_576;
const DEFAULT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(60);
const TOKEN_REFRESH_MARGIN_MS: i64 = 10_000;

#[derive(Clone)]
pub struct SondeClient {
    inner: Arc<Inner>,
}

struct Inner {
    http: HttpClient,
    endpoint: String,
    api_key: String,
    device_id: String,
    facts: RwLock<DeviceFacts>,
    token: Mutex<Option<TokenState>>,
    heartbeat_interval: Option<Duration>,
}

#[derive(Clone)]
struct TokenState {
    token: String,
    signing_key: String,
    expires_at: i64,
}

pub struct SondeClientBuilder {
    base_url: String,
    api_key: String,
    device_id: String,
    user_agent: String,
    facts: DeviceFacts,
    heartbeat_interval: Option<Duration>,
    request_timeout: Duration,
}

impl SondeClient {
    pub fn builder(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        device_id: impl Into<String>,
    ) -> SondeClientBuilder {
        SondeClientBuilder {
            base_url: base_url.into(),
            api_key: api_key.into(),
            device_id: device_id.into(),
            user_agent: format!("sonde-rust-sdk/{}", env!("CARGO_PKG_VERSION")),
            facts: DeviceFacts::with_platform_defaults(),
            heartbeat_interval: Some(DEFAULT_HEARTBEAT_INTERVAL),
            request_timeout: Duration::from_secs(10),
        }
    }

    pub async fn heartbeat(&self) -> Result<()> {
        let facts = self.inner.facts.read().await.clone();
        if facts.is_empty() {
            return Err(Error::InvalidConfiguration(
                "heartbeat requires at least one device fact".into(),
            ));
        }
        let body = serialize_payload(&facts)?;
        let response = self.signed_post("/api/v1/ingest/heartbeat", body).await?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(api_error(response).await)
        }
    }

    pub async fn set_device_facts(&self, facts: DeviceFacts) -> Result<()> {
        if facts.is_empty() {
            return Err(Error::InvalidConfiguration(
                "device facts must contain at least one value".into(),
            ));
        }
        *self.inner.facts.write().await = facts;
        self.heartbeat().await
    }

    pub async fn event(&self, event: Event) -> Result<BatchReceipt> {
        self.events(std::slice::from_ref(&event)).await
    }

    pub async fn events(&self, events: &[Event]) -> Result<BatchReceipt> {
        self.send_batch("/api/v1/ingest/events", events).await
    }

    pub async fn metric(&self, metric: Metric) -> Result<BatchReceipt> {
        self.metrics(std::slice::from_ref(&metric)).await
    }

    pub async fn metrics(&self, metrics: &[Metric]) -> Result<BatchReceipt> {
        self.send_batch("/api/v1/ingest/metrics", metrics).await
    }

    pub async fn log(&self, entry: LogEntry) -> Result<BatchReceipt> {
        self.logs(std::slice::from_ref(&entry)).await
    }

    pub async fn logs(&self, entries: &[LogEntry]) -> Result<BatchReceipt> {
        self.send_batch("/api/v1/ingest/logs", entries).await
    }

    pub async fn error(&self, error: ErrorEvent) -> Result<BatchReceipt> {
        self.errors(std::slice::from_ref(&error)).await
    }

    pub async fn errors(&self, errors: &[ErrorEvent]) -> Result<BatchReceipt> {
        self.send_batch("/api/v1/ingest/errors", errors).await
    }

    async fn send_batch<T: Serialize>(&self, path: &str, items: &[T]) -> Result<BatchReceipt> {
        if items.is_empty() || items.len() > MAX_BATCH_ITEMS {
            return Err(Error::InvalidBatchSize);
        }
        let body = serialize_payload(&BatchRef { items })?;
        if body.len() > MAX_INGEST_BODY_BYTES {
            return Err(Error::PayloadTooLarge);
        }
        let response = self.signed_post(path, body).await?;
        if !response.status().is_success() {
            return Err(api_error(response).await);
        }
        Ok(response.json::<BatchReceipt>().await?)
    }

    async fn signed_post(&self, path: &str, body: Vec<u8>) -> Result<Response> {
        let auth = self.token(false).await?;
        let first = self.signed_post_once(path, &body, &auth).await?;
        if first.status() != StatusCode::UNAUTHORIZED {
            return Ok(first);
        }

        self.invalidate_token().await;
        let refreshed = self.token(true).await?;
        self.signed_post_once(path, &body, &refreshed).await
    }

    async fn signed_post_once(
        &self,
        path: &str,
        body: &[u8],
        auth: &TokenState,
    ) -> Result<Response> {
        let timestamp = unix_millis()?;
        let nonce = Uuid::now_v7().to_string();
        let signature = signing::sign(
            &auth.signing_key,
            timestamp,
            &nonce,
            "POST",
            path,
            body,
        )?;

        Ok(self
            .inner
            .http
            .post(format!("{}{}", self.inner.endpoint, path))
            .bearer_auth(&auth.token)
            .header(CONTENT_TYPE, "application/json")
            .header("x-sonde-timestamp", timestamp.to_string())
            .header("x-sonde-nonce", nonce)
            .header("x-sonde-signature", signature)
            .body(body.to_vec())
            .send()
            .await?)
    }

    async fn token(&self, force_refresh: bool) -> Result<TokenState> {
        let now = unix_millis()?;
        let mut guard = self.inner.token.lock().await;
        if !force_refresh
            && let Some(token) = guard.as_ref()
            && token.expires_at.saturating_sub(now) > TOKEN_REFRESH_MARGIN_MS
        {
            return Ok(token.clone());
        }

        let response = self
            .inner
            .http
            .post(format!("{}/token", self.inner.endpoint))
            .bearer_auth(&self.inner.api_key)
            .json(&TokenRequest {
                device_id: &self.inner.device_id,
            })
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(api_error(response).await);
        }
        let issued = response.json::<TokenResponse>().await?;
        if issued.signature_version != SIGNATURE_VERSION {
            return Err(Error::UnsupportedSignatureVersion(issued.signature_version));
        }
        let token = TokenState {
            token: issued.token,
            signing_key: issued.signing_key,
            expires_at: issued.expires_at,
        };
        *guard = Some(token.clone());
        Ok(token)
    }

    async fn invalidate_token(&self) {
        *self.inner.token.lock().await = None;
    }

    fn start_automatic_heartbeat(&self) {
        let Some(period) = self.inner.heartbeat_interval else {
            return;
        };
        let weak = Arc::downgrade(&self.inner);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
            loop {
                interval.tick().await;
                let Some(inner) = weak.upgrade() else {
                    break;
                };
                let client = SondeClient { inner };
                let _ = client.heartbeat().await;
            }
        });
    }
}

impl SondeClientBuilder {
    pub fn user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = user_agent.into();
        self
    }

    pub fn app_version(mut self, version: impl Into<String>) -> Self {
        self.facts.app_version = Some(version.into());
        self
    }

    pub fn launcher_version(mut self, version: impl Into<String>) -> Self {
        self.facts.launcher_version = Some(version.into());
        self
    }

    pub fn os(mut self, os: impl Into<String>) -> Self {
        self.facts.os = Some(os.into());
        self
    }

    pub fn system_language(mut self, language: impl Into<String>) -> Self {
        self.facts.system_language = Some(language.into());
        self
    }

    pub fn architecture(mut self, architecture: impl Into<String>) -> Self {
        self.facts.architecture = Some(architecture.into());
        self
    }

    pub fn heartbeat_interval(mut self, interval: Duration) -> Self {
        self.heartbeat_interval = Some(interval);
        self
    }

    pub fn disable_automatic_heartbeat(mut self) -> Self {
        self.heartbeat_interval = None;
        self
    }

    pub fn request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    pub async fn connect(self) -> Result<SondeClient> {
        validate_nonempty("base URL", &self.base_url)?;
        validate_nonempty("API key", &self.api_key)?;
        validate_nonempty("device ID", &self.device_id)?;
        validate_nonempty("User-Agent", &self.user_agent)?;
        if self.heartbeat_interval.is_some_and(|interval| interval < Duration::from_secs(15)) {
            return Err(Error::InvalidConfiguration(
                "automatic heartbeat interval must be at least 15 seconds".into(),
            ));
        }

        let endpoint = normalize_endpoint(&self.base_url);
        let http = HttpClient::builder()
            .user_agent(self.user_agent)
            .timeout(self.request_timeout)
            .build()?;
        let client = SondeClient {
            inner: Arc::new(Inner {
                http,
                endpoint,
                api_key: self.api_key,
                device_id: self.device_id,
                facts: RwLock::new(self.facts),
                token: Mutex::new(None),
                heartbeat_interval: self.heartbeat_interval,
            }),
        };

        client.heartbeat().await?;
        client.start_automatic_heartbeat();
        Ok(client)
    }
}

fn normalize_endpoint(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.ends_with("/api/v1/ingest") {
        trimmed.to_owned()
    } else {
        format!("{trimmed}/api/v1/ingest")
    }
}

fn validate_nonempty(name: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(Error::InvalidConfiguration(format!("{name} must not be empty")));
    }
    Ok(())
}

fn serialize_payload<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(value)?)
}

fn unix_millis() -> Result<i64> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::InvalidClock)?;
    i64::try_from(elapsed.as_millis()).map_err(|_| Error::InvalidClock)
}

async fn api_error(response: Response) -> Error {
    let status = response.status();
    let body = match response.text().await {
        Ok(body) if !body.is_empty() => body,
        Ok(_) => status.canonical_reason().unwrap_or("Sonde request failed").to_owned(),
        Err(error) => format!("failed to read error response: {error}"),
    };
    Error::Api { status, body }
}

#[cfg(test)]
mod tests {
    use super::normalize_endpoint;

    #[test]
    fn normalizes_server_and_ingest_urls() {
        assert_eq!(normalize_endpoint("https://sonde.example.com/"), "https://sonde.example.com/api/v1/ingest");
        assert_eq!(normalize_endpoint("https://sonde.example.com/api/v1/ingest"), "https://sonde.example.com/api/v1/ingest");
    }
}
