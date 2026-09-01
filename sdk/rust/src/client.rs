use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use reqwest::{
    Client as HttpClient, Response, StatusCode,
    header::{CONTENT_TYPE, RETRY_AFTER},
};
use serde::Serialize;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

use crate::{
    delivery::{DeliveryOptions, DeliveryQueue, DeliveryStats, RetryPolicy, await_control},
    device_id::validate_device_id,
    error::{Error, Result},
    model::{
        BatchReceipt, DeviceFacts, ErrorEvent, Event, LogEntry, Metric, TokenRequest, TokenResponse,
    },
    signing::{self, SIGNATURE_VERSION},
    spool::SpoolOptions,
};

const MAX_BATCH_ITEMS: usize = 1_000;
const MAX_INGEST_BODY_BYTES: usize = 1_048_576;
const DEFAULT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(60);
const TOKEN_REFRESH_MARGIN_MS: i64 = 10_000;
const INGEST_PATH: &str = "/api/v1/ingest";
const BATCH_PREFIX: &[u8] = b"{\"items\":[";
const BATCH_SUFFIX: &[u8] = b"]}";

#[derive(Clone)]
pub struct SondeClient {
    inner: Arc<Inner>,
}

struct Inner {
    transport: Arc<Transport>,
    queues: Queues,
    heartbeat_interval: Option<Duration>,
    shutting_down: AtomicBool,
}

struct Queues {
    events: DeliveryQueue<Event>,
    metrics: DeliveryQueue<Metric>,
    logs: DeliveryQueue<LogEntry>,
    errors: DeliveryQueue<ErrorEvent>,
}

pub(crate) struct Transport {
    http: HttpClient,
    endpoint: String,
    api_key: String,
    device_id: String,
    facts: RwLock<DeviceFacts>,
    token: Mutex<Option<TokenState>>,
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
    delivery: DeliveryOptions,
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
            delivery: DeliveryOptions::default(),
        }
    }

    /// Send a heartbeat immediately. Normal heartbeat scheduling remains automatic after connect.
    pub async fn heartbeat(&self) -> Result<()> {
        self.ensure_running()?;
        self.inner.transport.heartbeat().await
    }

    /// Replace the current device facts and publish them immediately with a heartbeat.
    pub async fn set_device_facts(&self, facts: DeviceFacts) -> Result<()> {
        self.ensure_running()?;
        validate_device_facts(&facts)?;
        *self.inner.transport.facts.write().await = facts;
        self.inner.transport.heartbeat().await
    }

    /// Enqueue an event. With disk spooling enabled, completion means the item is fsynced to the
    /// queue WAL and admitted to the bounded in-memory worker queue. It does not mean the server has
    /// acknowledged the item; call `flush()` when an acknowledgement barrier is required.
    pub async fn event(&self, event: Event) -> Result<()> {
        self.ensure_running()?;
        self.inner.queues.events.enqueue(event).await
    }

    pub fn try_event(&self, event: Event) -> Result<()> {
        self.ensure_running()?;
        self.inner.queues.events.try_enqueue(event)
    }

    pub async fn events(&self, events: Vec<Event>) -> Result<()> {
        self.ensure_running()?;
        for event in events {
            self.inner.queues.events.enqueue(event).await?;
        }
        Ok(())
    }

    pub async fn metric(&self, metric: Metric) -> Result<()> {
        self.ensure_running()?;
        self.inner.queues.metrics.enqueue(metric).await
    }

    pub fn try_metric(&self, metric: Metric) -> Result<()> {
        self.ensure_running()?;
        self.inner.queues.metrics.try_enqueue(metric)
    }

    pub async fn metrics(&self, metrics: Vec<Metric>) -> Result<()> {
        self.ensure_running()?;
        for metric in metrics {
            self.inner.queues.metrics.enqueue(metric).await?;
        }
        Ok(())
    }

    pub async fn log(&self, entry: LogEntry) -> Result<()> {
        self.ensure_running()?;
        self.inner.queues.logs.enqueue(entry).await
    }

    pub fn try_log(&self, entry: LogEntry) -> Result<()> {
        self.ensure_running()?;
        self.inner.queues.logs.try_enqueue(entry)
    }

    pub async fn logs(&self, entries: Vec<LogEntry>) -> Result<()> {
        self.ensure_running()?;
        for entry in entries {
            self.inner.queues.logs.enqueue(entry).await?;
        }
        Ok(())
    }

    pub async fn error(&self, error: ErrorEvent) -> Result<()> {
        self.ensure_running()?;
        self.inner.queues.errors.enqueue(error).await
    }

    pub fn try_error(&self, error: ErrorEvent) -> Result<()> {
        self.ensure_running()?;
        self.inner.queues.errors.try_enqueue(error)
    }

    pub async fn errors(&self, errors: Vec<ErrorEvent>) -> Result<()> {
        self.ensure_running()?;
        for error in errors {
            self.inner.queues.errors.enqueue(error).await?;
        }
        Ok(())
    }

    /// Flush all four telemetry queues. The workers are independent, so all flush barriers are
    /// submitted before any one queue is awaited.
    pub async fn flush(&self) -> Result<()> {
        self.ensure_running()?;
        let requests = [
            ("events", self.inner.queues.events.request_flush().await),
            ("metrics", self.inner.queues.metrics.request_flush().await),
            ("logs", self.inner.queues.logs.request_flush().await),
            ("errors", self.inner.queues.errors.request_flush().await),
        ];
        await_requests(requests).await
    }

    /// Stop accepting new telemetry and perform a final flush of every queue.
    ///
    /// This method is idempotent. With disk spooling enabled, retryable batches that still cannot be
    /// delivered remain unacknowledged in the WAL and are replayed by the next client instance.
    pub async fn shutdown(&self) -> Result<()> {
        if self.inner.shutting_down.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let requests = [
            ("events", self.inner.queues.events.request_shutdown().await),
            ("metrics", self.inner.queues.metrics.request_shutdown().await),
            ("logs", self.inner.queues.logs.request_shutdown().await),
            ("errors", self.inner.queues.errors.request_shutdown().await),
        ];
        await_requests(requests).await
    }

    /// Snapshot delivery counters without acquiring the queue workers.
    pub fn delivery_stats(&self) -> DeliveryStats {
        DeliveryStats {
            events: self.inner.queues.events.stats(),
            metrics: self.inner.queues.metrics.stats(),
            logs: self.inner.queues.logs.stats(),
            errors: self.inner.queues.errors.stats(),
        }
    }

    fn ensure_running(&self) -> Result<()> {
        if self.inner.shutting_down.load(Ordering::Acquire) {
            Err(Error::ShuttingDown)
        } else {
            Ok(())
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
                if inner.shutting_down.load(Ordering::Acquire) {
                    break;
                }
                let _ = inner.transport.heartbeat().await;
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

    pub fn delivery_options(mut self, options: DeliveryOptions) -> Self {
        self.delivery = options;
        self
    }

    pub fn queue_capacity(mut self, capacity: usize) -> Self {
        self.delivery.queue_capacity = capacity;
        self
    }

    pub fn batch_size(mut self, max_items: usize) -> Self {
        self.delivery.max_batch_items = max_items;
        self
    }

    pub fn flush_interval(mut self, interval: Duration) -> Self {
        self.delivery.flush_interval = interval;
        self
    }

    pub fn retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.delivery.retry = policy;
        self
    }

    pub fn disk_spool(mut self, directory: impl Into<PathBuf>) -> Self {
        self.delivery.spool = Some(SpoolOptions::new(directory));
        self
    }

    pub fn spool_options(mut self, options: SpoolOptions) -> Self {
        self.delivery.spool = Some(options);
        self
    }

    pub fn disable_disk_spool(mut self) -> Self {
        self.delivery.spool = None;
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
        self.delivery.validate()?;
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
        let transport = Arc::new(Transport {
            http,
            endpoint,
            api_key: self.api_key,
            device_id,
            facts: RwLock::new(self.facts),
            token: Mutex::new(None),
        });

        // Fail fast on credentials/signature compatibility before opening durable queues.
        transport.heartbeat().await?;

        let heartbeat_interval = self.heartbeat_interval;
        let delivery = self.delivery;
        let queues = Queues {
            events: DeliveryQueue::spawn(transport.clone(), delivery.clone()).await?,
            metrics: DeliveryQueue::spawn(transport.clone(), delivery.clone()).await?,
            logs: DeliveryQueue::spawn(transport.clone(), delivery.clone()).await?,
            errors: DeliveryQueue::spawn(transport.clone(), delivery).await?,
        };
        let client = SondeClient {
            inner: Arc::new(Inner {
                transport,
                queues,
                heartbeat_interval,
                shutting_down: AtomicBool::new(false),
            }),
        };

        client.start_automatic_heartbeat();
        Ok(client)
    }
}

impl Transport {
    async fn heartbeat(&self) -> Result<()> {
        let facts = self.facts.read().await.clone();
        validate_device_facts(&facts)?;
        let body = serialize_payload(&facts)?;
        let response = self.signed_post("/heartbeat", body).await?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(api_error(response).await)
        }
    }

    pub(crate) async fn send_serialized_batch(
        &self,
        route: &str,
        items: &[&[u8]],
    ) -> Result<BatchReceipt> {
        if items.is_empty() || items.len() > MAX_BATCH_ITEMS {
            return Err(Error::InvalidBatchSize);
        }
        let body = build_serialized_batch_body(items)?;
        let response = self.signed_post(route, body).await?;
        if !response.status().is_success() {
            return Err(api_error(response).await);
        }
        Ok(response.json::<BatchReceipt>().await?)
    }

    async fn signed_post(&self, route: &str, body: Vec<u8>) -> Result<Response> {
        let auth = self.token().await?;
        let first = self.signed_post_once(route, &body, &auth).await?;
        if first.status() != StatusCode::UNAUTHORIZED {
            return Ok(first);
        }

        self.invalidate_token_if(&auth.token).await;
        let refreshed = self.token().await?;
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
            .http
            .post(format!("{}{}", self.endpoint, route))
            .bearer_auth(&auth.token)
            .header(CONTENT_TYPE, "application/json")
            .header("x-sonde-timestamp", timestamp.to_string())
            .header("x-sonde-nonce", nonce)
            .header("x-sonde-signature", signature)
            .body(body.to_vec())
            .send()
            .await?)
    }

    async fn token(&self) -> Result<TokenState> {
        let now = unix_millis()?;
        let mut guard = self.token.lock().await;
        if let Some(token) = guard.as_ref()
            && token.expires_at.saturating_sub(now) > TOKEN_REFRESH_MARGIN_MS
        {
            return Ok(token.clone());
        }

        let response = self
            .http
            .post(format!("{}/token", self.endpoint))
            .bearer_auth(&self.api_key)
            .json(&TokenRequest {
                device_id: &self.device_id,
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
        let mut guard = self.token.lock().await;
        if guard
            .as_ref()
            .is_some_and(|current| current.token == stale_token)
        {
            *guard = None;
        }
    }
}

async fn await_requests(
    requests: [
        (
            &'static str,
            Result<tokio::sync::oneshot::Receiver<Result<()>>>,
        );
        4
    ],
) -> Result<()> {
    let mut first_error = None;
    for (kind, request) in requests {
        let result = match request {
            Ok(receiver) => await_control(receiver, kind).await,
            Err(error) => Err(error),
        };
        if first_error.is_none()
            && let Err(error) = result
        {
            first_error = Some(error);
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn build_serialized_batch_body(items: &[&[u8]]) -> Result<Vec<u8>> {
    let payload_bytes = items.iter().try_fold(0_usize, |total, item| {
        total.checked_add(item.len()).ok_or(Error::PayloadTooLarge)
    })?;
    let separators = items.len().saturating_sub(1);
    let total_len = BATCH_PREFIX
        .len()
        .checked_add(payload_bytes)
        .and_then(|value| value.checked_add(separators))
        .and_then(|value| value.checked_add(BATCH_SUFFIX.len()))
        .ok_or(Error::PayloadTooLarge)?;
    if total_len > MAX_INGEST_BODY_BYTES {
        return Err(Error::PayloadTooLarge);
    }

    let mut body = Vec::with_capacity(total_len);
    body.extend_from_slice(BATCH_PREFIX);
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            body.push(b',');
        }
        body.extend_from_slice(item);
    }
    body.extend_from_slice(BATCH_SUFFIX);
    Ok(body)
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
    let retry_after = response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(Duration::from_secs);
    let body = match response.text().await {
        Ok(body) if !body.is_empty() => body,
        Ok(_) => status
            .canonical_reason()
            .unwrap_or("Sonde request failed")
            .to_owned(),
        Err(error) => format!("failed to read error response: {error}"),
    };
    Error::Api {
        status,
        body,
        retry_after,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        INGEST_PATH, build_serialized_batch_body, normalize_endpoint, validate_device_facts,
        validate_user_agent,
    };
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
    fn rebuilds_batch_from_serialized_items_without_reserializing() -> crate::Result<()> {
        let first = br#"{\"name\":\"a\"}"#;
        let second = br#"{\"name\":\"b\"}"#;
        let body = build_serialized_batch_body(&[first.as_slice(), second.as_slice()])?;
        assert_eq!(body, br#"{\"items\":[{\"name\":\"a\"},{\"name\":\"b\"}]}"#);
        Ok(())
    }

    #[test]
    fn validates_server_owned_device_facts_contract() {
        assert!(validate_device_facts(&DeviceFacts::with_platform_defaults()).is_ok());
        assert!(validate_device_facts(&DeviceFacts::default()).is_err());
        assert!(validate_user_agent("sonde-rust-sdk/test").is_ok());
        assert!(validate_user_agent("bad\nagent").is_err());
    }
}
