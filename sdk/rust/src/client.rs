use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use reqwest::{header::CONTENT_TYPE, Client as HttpClient, Response, StatusCode};
use serde::Serialize;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

use crate::{
    device_id::validate_device_id,
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
const INGEST_PATH: &str = "/api/v1/ingest";

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
        validate_device_facts(&facts)?;
        let body = serialize_payload(&facts)?;
        let response = self.signed_post("/heartbeat", body).await?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(api_error(response).await)
        }
    }

    pub async fn set_device_facts(&self, facts: DeviceFacts) -> Result<()> {
        validate_device_facts(&facts)?;
        *self.inner.facts.write().await = facts;
        self.heartbeat().await
    }

    pub async fn event(&self, event: Event) -> Result<BatchReceipt> {
        self.events(std::slice::from_ref(&event)).await
    }

    pub async fn events(&self, events: &[Event]) -> Result<BatchReceipt> {
        self.send_batch("/events", events).await
    }

    pub async fn metric(&self, metric: Metric) -> Result<BatchReceipt> {
        self.metrics(std::slice::from_ref(&metric)).await
    }

    pub async fn metrics(&self, metrics: &[Metric]) -> Result<BatchReceipt> {
        self.send_batch("/metrics", metrics).await
    }

    pub async fn log(&self, entry: LogEntry) -> Result<BatchReceipt> {
        self.logs(std::slice::from_ref(&entry)).await
    }

    pub async fn logs(&self, entries: &[LogEntry]) -> Result<BatchReceipt> {
        self.send_batch("/logs", entries).await
    }

    pub async fn error(&self, error: ErrorEvent) -> Result<BatchReceipt> {
        self.errors(std::slice::from_ref(&error)).await
    }

    pub async fn errors(&self, errors: &[ErrorEvent]) -> Result<BatchReceipt> {
        self.send_batch("/errors", errors).await
    }

    async fn send_batch<T: Serialize>(&self, route: &str, items: &[T]) -> Result<BatchReceipt> {
        if items.is_empty() || items.len() > MAX_BATCH_ITEMS {
            return Err(Error::InvalidBatchSize);
        }
        let body = serialize_payload(&BatchRef { items })?;
        if body.len() > MAX_INGEST_BODY_BYTES {
            return Err(Error::PayloadTooLarge);
        }
        let response = self.signed_post(route, body).await?;
        if !response.status().is_success() {
            return Err(api_error(response).await);
        }
        Ok(response.json::<BatchReceipt>().await?)
    }

    async fn signed_post(&self, route: &str, body: Vec<u8>) -> Result<Response> {
        let auth = self.token(false).await?;
        let first = self.signed_post_once(route, &body, &auth).await?;
        if first.status() != StatusCode::UNAUTHORIZED {
            return Ok(first);
        }

        self.invalidate_token_if(&auth.token).await;
        let refreshed = self.token(false).await?;
        self.signed_post_once(route, &body, &refreshed).await
    }

    async fn signed_post_once(
        &self,
        route: &str,
        body: &[u8],
        auth: &TokenState,
    ) -> Result<Response> {
        let timestamp = unix_millis()?;
        let nonce = Uuid::now_v7().to_string();
        let canonical_path = format!("{INGEST_PATH}{route}");
        let signature = signing::sign(
            &auth.signing_key,
            timestamp,
            &nonce,
            "POST",
            &canonical_path,
            body,
        )?;

        Ok(self
            .inner
            .http
            .post(format!("{}{}", self.inner.endpoint, route))
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

    async fn invalidate_token_if(&self, stale_token: &str) {
        let mut guard = self.inner.token.lock().await;
        if guard
            .as_ref()
            .is_some_and(|current| current.token == stale_token)
        {
            *guard = None;
        }
    }

    fn start_automatic_heartbeat(&self) {
        let Some(period) = self.inner.heartbeat_interval else {
            return;
        };
        let weak = Arc::downgrade(&self.inner);
        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval_at(tokio::time::Instant::now() + period, period);
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
        validate_user_agent(&self.user_agent)?;
        let device_id = validate_device_id(&self.device_id)
            .map_err(|message| Error::InvalidConfiguration(message.into()))?
            .to_owned();
        validate_device_facts(&self.facts)?;
        if self
            .heartbeat_interval
            .is_some_and(|interval| interval < Duration::from_secs(15))
        {
            return Err(Error::InvalidConfiguration(
                "automatic heartbeat interval must be at least 15 seconds".into(),
            ));
        }
        if self.request_timeout.is_zero() {
            return Err(Error::InvalidConfiguration(
                "request timeout must be greater than zero".into(),
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
                device_id,
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
    if trimmed.ends_with(INGEST_PATH) {
        trimmed.to_owned()
    } else {
        format!("{trimmed}{INGEST_PATH}")
    }
}

fn validate_nonempty(name: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(Error::InvalidConfiguration(format!(
            "{name} must not be empty"
        )));
    }
    Ok(())
}

fn validate_user_agent(value: &str) -> Result<()> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 512
        || value.chars().any(|character| character.is_control())
    {
        return Err(Error::InvalidConfiguration(
            "User-Agent must be 1..512 visible bytes".into(),
        ));
    }
    Ok(())
}

fn validate_device_facts(facts: &DeviceFacts) -> Result<()> {
    if facts.is_empty() {
        return Err(Error::InvalidConfiguration(
            "heartbeat requires at least one device fact".into(),
        ));
    }
    validate_optional_fact(&facts.app_version, 128, "appVersion")?;
    validate_optional_fact(&facts.launcher_version, 128, "launcherVersion")?;
    validate_optional_fact(&facts.os, 256, "os")?;
    validate_optional_fact(&facts.system_language, 64, "systemLanguage")?;
    validate_optional_fact(&facts.architecture, 64, "architecture")?;
    Ok(())
}

fn validate_optional_fact(value: &Option<String>, max_bytes: usize, name: &str) -> Result<()> {
    if let Some(value) = value
        && (value.is_empty()
            || value.len() > max_bytes
            || value.chars().any(|character| character.is_control()))
    {
        return Err(Error::InvalidConfiguration(format!(
            "{name} must be 1..{max_bytes} visible bytes when provided"
        )));
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
        Ok(_) => status
            .canonical_reason()
            .unwrap_or("Sonde request failed")
            .to_owned(),
        Err(error) => format!("failed to read error response: {error}"),
    };
    Error::Api { status, body }
}

#[cfg(test)]
mod tests {
    use super::{INGEST_PATH, normalize_endpoint, validate_device_facts, validate_user_agent};
    use crate::model::DeviceFacts;

    #[test]
    fn normalizes_server_and_ingest_urls() {
        assert_eq!(
            normalize_endpoint("https://sonde.example.com/"),
            "https://sonde.example.com/api/v1/ingest"
        );
        assert_eq!(
            normalize_endpoint("https://sonde.example.com/api/v1/ingest"),
            "https://sonde.example.com/api/v1/ingest"
        );
        assert_eq!(INGEST_PATH, "/api/v1/ingest");
    }

    #[test]
    fn validates_server_owned_device_facts_contract() {
        assert!(validate_device_facts(&DeviceFacts::with_platform_defaults()).is_ok());
        assert!(validate_device_facts(&DeviceFacts::default()).is_err());
        assert!(validate_user_agent("sonde-rust-sdk/test").is_ok());
        assert!(validate_user_agent("bad\nagent").is_err());
    }
}
