use std::collections::HashMap;
use std::sync::Arc;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tokio::sync::Mutex;

use crate::{auth, error::AppError};

const CHALLENGE_MILLIS: i64 = 5 * 60 * 1_000;
const INGEST_SIGNING_CONTEXT: &[u8] = b"sonde-ingest-signing-v1\n";
const INGEST_CLIENT_BINDING_CONTEXT: &[u8] = b"sonde-ingest-client-binding-v1\n";
const RATE_WINDOW_MILLIS: i64 = 60_000;
const IP_INGEST_REQUESTS_PER_MINUTE: u64 = 120;
const IP_INGEST_BYTES_PER_MINUTE: u64 = 8 * 1024 * 1024;
const IP_INGEST_ITEMS_PER_MINUTE: u64 = 10_000;
const DEVICE_INGEST_REQUESTS_PER_MINUTE: u64 = 60;
const DEVICE_INGEST_BYTES_PER_MINUTE: u64 = 2 * 1024 * 1024;
const DEVICE_INGEST_ITEMS_PER_MINUTE: u64 = 2_000;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Default)]
struct Attempt {
    failures: u32,
    next_allowed_at: i64,
}

#[derive(Debug)]
struct Challenge {
    account_key: String,
    source_key: String,
    expires_at: i64,
}

#[derive(Debug)]
pub enum LoginGate {
    Allowed,
    ChallengeRequired { challenge_id: String },
    Delayed { retry_after_seconds: u64 },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct IngestTokenClaims {
    pub application_id: String,
    pub environment_id: String,
    pub device_id: String,
    pub client_binding: String,
    pub scopes: Vec<String>,
    pub issued_at: i64,
    pub expires_at: i64,
    pub token_id: String,
}

#[derive(Debug)]
struct RateBucket {
    count: u64,
    window_start: i64,
}

impl RateBucket {
    fn new(now: i64, cost: u64) -> Self {
        Self {
            count: cost,
            window_start: now,
        }
    }

    fn charge(&mut self, now: i64, cost: u64, limit: u64, window_ms: i64) -> bool {
        if cost > limit {
            return false;
        }
        if now.saturating_sub(self.window_start) >= window_ms {
            self.count = cost;
            self.window_start = now;
            return true;
        }
        let next = self.count.saturating_add(cost);
        if next > limit {
            return false;
        }
        self.count = next;
        true
    }
}

pub struct IngestSecurity {
    ip_ingest_rate: Mutex<HashMap<String, RateBucket>>,
    ip_ingest_bytes: Mutex<HashMap<String, RateBucket>>,
    ip_ingest_items: Mutex<HashMap<String, RateBucket>>,
    device_rate: Mutex<HashMap<String, RateBucket>>,
    device_ingest_bytes: Mutex<HashMap<String, RateBucket>>,
    device_ingest_items: Mutex<HashMap<String, RateBucket>>,
}

impl Default for IngestSecurity {
    fn default() -> Self {
        Self::new()
    }
}

impl IngestSecurity {
    pub fn new() -> Self {
        Self {
            ip_ingest_rate: Mutex::new(HashMap::new()),
            ip_ingest_bytes: Mutex::new(HashMap::new()),
            ip_ingest_items: Mutex::new(HashMap::new()),
            device_rate: Mutex::new(HashMap::new()),
            device_ingest_bytes: Mutex::new(HashMap::new()),
            device_ingest_items: Mutex::new(HashMap::new()),
        }
    }

    pub async fn check_ip_ingest_rate(&self, ip: &str) -> bool {
        charge_rate(
            &self.ip_ingest_rate,
            ip,
            1,
            IP_INGEST_REQUESTS_PER_MINUTE,
            RATE_WINDOW_MILLIS,
            20_000,
        )
        .await
    }

    pub async fn check_ip_ingest_bytes(&self, ip: &str, body_bytes: usize) -> bool {
        charge_rate(
            &self.ip_ingest_bytes,
            ip,
            body_bytes.max(1) as u64,
            IP_INGEST_BYTES_PER_MINUTE,
            RATE_WINDOW_MILLIS,
            20_000,
        )
        .await
    }

    pub async fn check_ip_ingest_items(&self, ip: &str, item_count: usize) -> bool {
        charge_rate(
            &self.ip_ingest_items,
            ip,
            item_count.max(1) as u64,
            IP_INGEST_ITEMS_PER_MINUTE,
            RATE_WINDOW_MILLIS,
            20_000,
        )
        .await
    }

    pub async fn check_device_rate(&self, app_id: &str, device_id: &str) -> bool {
        charge_device_rate(
            &self.device_rate,
            app_id,
            device_id,
            1,
            DEVICE_INGEST_REQUESTS_PER_MINUTE,
        )
        .await
    }

    pub async fn check_device_ingest_bytes(
        &self,
        app_id: &str,
        device_id: &str,
        body_bytes: usize,
    ) -> bool {
        charge_device_rate(
            &self.device_ingest_bytes,
            app_id,
            device_id,
            body_bytes.max(1) as u64,
            DEVICE_INGEST_BYTES_PER_MINUTE,
        )
        .await
    }

    pub async fn check_device_ingest_items(
        &self,
        app_id: &str,
        device_id: &str,
        item_count: usize,
    ) -> bool {
        charge_device_rate(
            &self.device_ingest_items,
            app_id,
            device_id,
            item_count.max(1) as u64,
            DEVICE_INGEST_ITEMS_PER_MINUTE,
        )
        .await
    }

    pub fn client_binding(&self, user_agent: &str, pepper: &[u8]) -> Result<String, AppError> {
        let mut context =
            Vec::with_capacity(INGEST_CLIENT_BINDING_CONTEXT.len() + user_agent.len());
        context.extend_from_slice(INGEST_CLIENT_BINDING_CONTEXT);
        context.extend_from_slice(user_agent.as_bytes());
        Ok(hex::encode(hmac_sha256(pepper, &context)?))
    }

    /// Issue a short-lived ingest token bound only to the current pseudonymous device identity and
    /// transport client profile. Version/session/OS history stays in telemetry and is platform-owned.
    #[allow(clippy::too_many_arguments)]
    pub fn issue_ingest_token(
        &self,
        application_id: &str,
        environment_id: &str,
        device_id: &str,
        client_binding: &str,
        scopes: &[String],
        ttl_seconds: i64,
        pepper: &[u8],
    ) -> Result<(String, String, i64), AppError> {
        let now = chrono::Utc::now().timestamp_millis();
        let expires_at = now.saturating_add(ttl_seconds.saturating_mul(1_000));
        let token_id = auth::random_token(18);
        let signing_key = derive_ingest_signing_key(pepper, &token_id)?;

        let claims = IngestTokenClaims {
            application_id: application_id.to_owned(),
            environment_id: environment_id.to_owned(),
            device_id: device_id.to_owned(),
            client_binding: client_binding.to_owned(),
            scopes: scopes.to_vec(),
            issued_at: now,
            expires_at,
            token_id,
        };

        let json_bytes = serde_json::to_vec(&claims)
            .map_err(|error| AppError::internal("serialize ingest token claims", error))?;
        let payload_b64 = URL_SAFE_NO_PAD.encode(json_bytes);
        let sig = hmac_sha256(pepper, payload_b64.as_bytes())?;
        let token = format!("sndt_{}.{}", payload_b64, hex::encode(sig));

        Ok((token, signing_key, expires_at))
    }

    pub fn verify_ingest_token(&self, token_str: &str, pepper: &[u8]) -> Option<IngestTokenClaims> {
        let without_prefix = token_str.strip_prefix("sndt_")?;
        let (payload_b64, sig_hex) = without_prefix.split_once('.')?;
        let provided_sig = hex::decode(sig_hex).ok()?;
        if !verify_hmac_sha256(pepper, payload_b64.as_bytes(), &provided_sig) {
            return None;
        }

        let json_bytes = URL_SAFE_NO_PAD.decode(payload_b64).ok()?;
        let claims: IngestTokenClaims = serde_json::from_slice(&json_bytes).ok()?;

        let now = chrono::Utc::now().timestamp_millis();
        let lifetime = claims.expires_at.saturating_sub(claims.issued_at);
        if claims.expires_at <= now
            || claims.issued_at > now.saturating_add(60_000)
            || lifetime <= 0
            || lifetime > 5 * 60 * 1_000
            || claims.token_id.is_empty()
            || claims.device_id.is_empty()
            || claims.client_binding.is_empty()
        {
            return None;
        }

        Some(claims)
    }

    pub fn signing_key_for_claims(
        &self,
        claims: &IngestTokenClaims,
        pepper: &[u8],
    ) -> Result<String, AppError> {
        derive_ingest_signing_key(pepper, &claims.token_id)
    }
}

async fn charge_rate(
    buckets: &Mutex<HashMap<String, RateBucket>>,
    key: &str,
    cost: u64,
    limit: u64,
    window_ms: i64,
    max_entries: usize,
) -> bool {
    let now = chrono::Utc::now().timestamp_millis();
    let mut buckets = buckets.lock().await;
    if buckets.len() > max_entries {
        buckets.retain(|_, bucket| now.saturating_sub(bucket.window_start) < window_ms);
    }
    if let Some(bucket) = buckets.get_mut(key) {
        bucket.charge(now, cost, limit, window_ms)
    } else if cost <= limit {
        buckets.insert(key.to_owned(), RateBucket::new(now, cost));
        true
    } else {
        false
    }
}

async fn charge_device_rate(
    buckets: &Mutex<HashMap<String, RateBucket>>,
    app_id: &str,
    device_id: &str,
    cost: u64,
    limit: u64,
) -> bool {
    let total = app_id
        .len()
        .saturating_add(device_id.len())
        .saturating_add(1);
    if total <= 384 {
        let mut buffer = [0_u8; 384];
        buffer[..app_id.len()].copy_from_slice(app_id.as_bytes());
        buffer[app_id.len()] = b':';
        buffer[app_id.len() + 1..total].copy_from_slice(device_id.as_bytes());
        if let Ok(key) = std::str::from_utf8(&buffer[..total]) {
            return charge_rate(buckets, key, cost, limit, RATE_WINDOW_MILLIS, 50_000).await;
        }
    }
    let mut key = String::with_capacity(total);
    key.push_str(app_id);
    key.push(':');
    key.push_str(device_id);
    charge_rate(buckets, &key, cost, limit, RATE_WINDOW_MILLIS, 50_000).await
}

fn derive_ingest_signing_key(pepper: &[u8], token_id: &str) -> Result<String, AppError> {
    let mut context = Vec::with_capacity(INGEST_SIGNING_CONTEXT.len() + token_id.len());
    context.extend_from_slice(INGEST_SIGNING_CONTEXT);
    context.extend_from_slice(token_id.as_bytes());
    Ok(format!(
        "sec_{}",
        URL_SAFE_NO_PAD.encode(hmac_sha256(pepper, &context)?)
    ))
}

fn verify_hmac_sha256(key: &[u8], data: &[u8], provided: &[u8]) -> bool {
    let Ok(mut mac) = HmacSha256::new_from_slice(key) else {
        return false;
    };
    mac.update(data);
    mac.verify_slice(provided).is_ok()
}

pub fn hmac_sha256(key: &[u8], data: &[u8]) -> Result<[u8; 32], AppError> {
    let mut mac = HmacSha256::new_from_slice(key)
        .map_err(|error| AppError::internal("initialize hmac-sha256", error))?;
    mac.update(data);
    let bytes = mac.finalize().into_bytes();
    let mut output = [0_u8; 32];
    output.copy_from_slice(&bytes);
    Ok(output)
}

/// Fixed-window throttle for the unauthenticated setup wizard, which runs before any
/// installation (and therefore before `AuthSecurity`) exists. Without it, an exposed
/// not-yet-installed instance is a convenient probe for reachable database endpoints.
pub struct PreInstallThrottle {
    buckets: Mutex<HashMap<String, RateBucket>>,
    limit: u64,
    window_ms: i64,
}

impl PreInstallThrottle {
    #[must_use]
    pub fn new(limit: u64, window_ms: i64) -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
            limit,
            window_ms,
        }
    }

    /// Charges one request against `key`, returning `false` once the window budget is spent.
    pub async fn charge(&self, key: &str, max_entries: usize) -> bool {
        charge_rate(
            &self.buckets,
            key,
            1,
            self.limit,
            self.window_ms,
            max_entries,
        )
        .await
    }
}

pub struct AuthSecurity {
    attempts: Mutex<HashMap<String, Attempt>>,
    challenges: Mutex<HashMap<String, Challenge>>,
    pub ingest: Arc<IngestSecurity>,
    pepper: Vec<u8>,
    dummy_password_hash: String,
}

impl AuthSecurity {
    pub fn new(pepper: &[u8]) -> Result<Self, AppError> {
        Ok(Self {
            attempts: Mutex::new(HashMap::new()),
            challenges: Mutex::new(HashMap::new()),
            ingest: Arc::new(IngestSecurity::new()),
            pepper: pepper.to_vec(),
            dummy_password_hash: auth::hash_password("sonde-dummy-credential-never-used", pepper)?,
        })
    }

    pub fn pepper(&self) -> &[u8] {
        &self.pepper
    }

    pub fn dummy_password_hash(&self) -> &str {
        &self.dummy_password_hash
    }

    pub async fn login_gate(
        &self,
        account_key: &str,
        source_key: &str,
        challenge_id: Option<&str>,
    ) -> LoginGate {
        let now = chrono::Utc::now().timestamp_millis();
        let attempts = self.attempts.lock().await;
        let account = attempts.get(account_key);
        let source = attempts.get(source_key);
        let next_allowed_at = account
            .map_or(0, |attempt| attempt.next_allowed_at)
            .max(source.map_or(0, |attempt| attempt.next_allowed_at));
        if next_allowed_at > now {
            return LoginGate::Delayed {
                retry_after_seconds: ((next_allowed_at - now) as u64).div_ceil(1_000),
            };
        }
        let requires_challenge = account.is_some_and(|attempt| attempt.failures > 0)
            || source.is_some_and(|attempt| attempt.failures > 0);
        drop(attempts);
        if !requires_challenge {
            return LoginGate::Allowed;
        }
        if self
            .consume_challenge(account_key, source_key, challenge_id, now)
            .await
        {
            LoginGate::Allowed
        } else {
            self.issue_challenge(account_key, source_key, now).await
        }
    }

    pub async fn record_failure(&self, account_key: &str, source_key: &str) -> LoginGate {
        let now = chrono::Utc::now().timestamp_millis();
        self.bump_attempts(&[account_key, source_key], now).await;
        self.issue_challenge(account_key, source_key, now).await
    }

    /// Reports how long the client must wait before another second-factor attempt is accepted.
    ///
    /// Second-factor verification needs its own throttle: a valid password yields a fresh
    /// pending token on every sign-in, so the sign-in gate alone cannot bound how fast an
    /// attacker walks the six-digit code space.
    pub async fn two_factor_gate(&self, keys: &[&str]) -> Option<u64> {
        let now = chrono::Utc::now().timestamp_millis();
        let attempts = self.attempts.lock().await;
        let next_allowed_at = keys
            .iter()
            .filter_map(|key| attempts.get(*key))
            .map(|attempt| attempt.next_allowed_at)
            .max()
            .unwrap_or(0);
        (next_allowed_at > now).then(|| ((next_allowed_at - now) as u64).div_ceil(1_000))
    }

    /// Records a rejected second-factor attempt against the account and the request source.
    pub async fn record_two_factor_failure(&self, keys: &[&str]) -> u64 {
        let now = chrono::Utc::now().timestamp_millis();
        self.bump_attempts(keys, now).await
    }

    async fn bump_attempts(&self, keys: &[&str], now: i64) -> u64 {
        let mut attempts = self.attempts.lock().await;
        if attempts.len() > 20_000 {
            attempts.retain(|_, a| a.next_allowed_at > now - 86_400_000);
        }
        let mut cooldown_seconds = 0_u64;
        for key in keys {
            let attempt = attempts.entry((*key).to_owned()).or_default();
            attempt.failures = attempt.failures.saturating_add(1);
            let delay_seconds = attempt
                .failures
                .checked_sub(2)
                .map_or(0, |power| 2_u64.saturating_pow(power).min(300));
            attempt.next_allowed_at = now + delay_seconds as i64 * 1_000;
            cooldown_seconds = cooldown_seconds.max(delay_seconds);
        }
        cooldown_seconds
    }

    pub async fn record_success(&self, account_key: &str, source_key: &str) {
        let mut attempts = self.attempts.lock().await;
        attempts.remove(account_key);
        attempts.remove(source_key);
    }

    async fn issue_challenge(&self, account_key: &str, source_key: &str, now: i64) -> LoginGate {
        let challenge_id = auth::random_token(18);
        let mut challenges = self.challenges.lock().await;
        if challenges.len() > 20_000 {
            challenges.retain(|_, c| c.expires_at > now);
        }
        challenges.insert(
            challenge_id.clone(),
            Challenge {
                account_key: account_key.to_owned(),
                source_key: source_key.to_owned(),
                expires_at: now + CHALLENGE_MILLIS,
            },
        );
        LoginGate::ChallengeRequired { challenge_id }
    }

    async fn consume_challenge(
        &self,
        account_key: &str,
        source_key: &str,
        challenge_id: Option<&str>,
        now: i64,
    ) -> bool {
        let Some(challenge_id) = challenge_id else {
            return false;
        };
        let mut challenges = self.challenges.lock().await;
        let Some(challenge) = challenges.remove(challenge_id) else {
            return false;
        };
        challenge.expires_at > now
            && challenge.account_key == account_key
            && challenge.source_key == source_key
    }
}

/// Upper bound for page-based pagination. Deep `OFFSET` scans are expensive, and an unbounded
/// page number also overflows the offset computation.
pub const MAX_PAGE: u64 = 10_000;

/// Clamps a caller-supplied page number into `1..=MAX_PAGE`.
#[must_use]
pub fn bounded_page(page: Option<u64>) -> u64 {
    page.unwrap_or(1).clamp(1, MAX_PAGE)
}

/// Clamps a caller-supplied page size into `1..=200`.
#[must_use]
pub fn bounded_page_size(page_size: Option<u64>) -> u64 {
    page_size.unwrap_or(50).clamp(1, 200)
}

/// Validates that an identifier (application ID, environment ID, key ID, user ID, slug, etc.)
/// is non-empty, within max_len (1..128 bytes), and strictly contains only safe characters
/// (`[a-zA-Z0-9_.-]`), does not contain path traversal sequences like `..`, `/`, `\`,
/// null bytes, control characters, and does not start with `.` or `-`.
pub fn validate_safe_identifier(field: &str, value: &str) -> Result<String, AppError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > 128 {
        return Err(AppError::Validation(format!(
            "{field} must be 1..128 characters"
        )));
    }
    // Disallow starting with dot, hyphen or slash
    if trimmed.starts_with('.')
        || trimmed.starts_with('-')
        || trimmed.starts_with('/')
        || trimmed.starts_with('\\')
    {
        return Err(AppError::Validation(format!(
            "{field} must start with an alphanumeric character or underscore"
        )));
    }
    // Disallow path traversal, slashes, nulls
    if trimmed.contains("..")
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains('\0')
    {
        return Err(AppError::Validation(format!(
            "{field} contains invalid or traversal characters"
        )));
    }
    // Whitelist character set: [a-zA-Z0-9_.-]
    for ch in trimmed.chars() {
        if !ch.is_ascii_alphanumeric() && ch != '_' && ch != '-' && ch != '.' {
            return Err(AppError::Validation(format!(
                "{field} contains forbidden character '{ch}'"
            )));
        }
    }
    Ok(trimmed.to_owned())
}

/// Validates an optional identifier, returning Ok(None) if empty or None,
/// or Ok(Some(safe_id)) if present and valid.
pub fn validate_optional_safe_identifier(
    field: &str,
    value: Option<&str>,
) -> Result<Option<String>, AppError> {
    match value.map(str::trim).filter(|v| !v.is_empty()) {
        None => Ok(None),
        Some(v) => validate_safe_identifier(field, v).map(Some),
    }
}

/// Validates a relative file/asset path to ensure it cannot escape its base directory via path traversal.
pub fn validate_safe_relative_path(raw: &str) -> Result<String, AppError> {
    if raw.is_empty() {
        return Ok(String::new());
    }
    // Reject explicit path traversal tokens, null bytes, backslashes, percent encoding tricks
    if raw.contains('\0')
        || raw.contains('\\')
        || raw.contains("%2e")
        || raw.contains("%2E")
        || raw.contains("%2f")
        || raw.contains("%2F")
        || raw.contains("%5c")
        || raw.contains("%5C")
        || raw.contains("%00")
        || raw.contains("%25")
    // reject double percent-encoding
    {
        return Err(AppError::Validation(
            "invalid path characters or traversal detected".into(),
        ));
    }
    let path = std::path::Path::new(raw.trim_start_matches('/'));
    for component in path.components() {
        match component {
            std::path::Component::Normal(_) | std::path::Component::CurDir => {}
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => {
                return Err(AppError::Validation(
                    "path traversal component detected".into(),
                ));
            }
        }
    }
    Ok(raw.to_owned())
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
pub mod tests {
    use super::*;

    #[test]
    fn first_failure_requires_a_one_time_challenge() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let security = AuthSecurity::new(b"pepper").unwrap();
            assert!(matches!(
                security.login_gate("admin", "127.0.0.1", None).await,
                LoginGate::Allowed
            ));

            let challenge = security.record_failure("admin", "127.0.0.1").await;
            let LoginGate::ChallengeRequired { challenge_id } = challenge else {
                panic!("expected challenge");
            };

            assert!(matches!(
                security
                    .login_gate("admin", "127.0.0.1", Some(&challenge_id))
                    .await,
                LoginGate::Allowed
            ));
        });
    }

    #[test]
    fn ingest_token_is_signed_and_bound_to_device_and_client() {
        let pepper = b"test-secret-pepper-32-bytes-long!";
        let ingest = IngestSecurity::new();
        let user_agent = "SondeTest/1.0";
        let client_binding = ingest.client_binding(user_agent, pepper).unwrap();

        let (token, signing_key, expires_at) = ingest
            .issue_ingest_token(
                "app-uuid-1",
                "env-uuid-1",
                "device-12345",
                &client_binding,
                &["telemetry.events".into(), "telemetry.errors".into()],
                120,
                pepper,
            )
            .unwrap();

        assert!(token.starts_with("sndt_"));
        let claims = ingest.verify_ingest_token(&token, pepper).unwrap();
        assert_eq!(claims.application_id, "app-uuid-1");
        assert_eq!(claims.device_id, "device-12345");
        assert_eq!(claims.client_binding, client_binding);
        assert_eq!(
            ingest.signing_key_for_claims(&claims, pepper).unwrap(),
            signing_key
        );
        assert_eq!(claims.expires_at, expires_at);
        assert!(claims.scopes.contains(&"telemetry.events".to_string()));

        let payload_b64 = token
            .strip_prefix("sndt_")
            .and_then(|value| value.split_once('.'))
            .map(|(payload, _)| payload)
            .unwrap();
        let decoded = String::from_utf8(URL_SAFE_NO_PAD.decode(payload_b64).unwrap()).unwrap();
        assert!(!decoded.contains(&signing_key));
        assert!(!decoded.contains(user_agent));
    }

    #[test]
    fn large_signed_batches_consume_device_byte_budget() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let ingest = IngestSecurity::new();
            assert!(
                ingest
                    .check_device_ingest_bytes("app", "device", 1024 * 1024)
                    .await
            );
            assert!(
                ingest
                    .check_device_ingest_bytes("app", "device", 1024 * 1024)
                    .await
            );
            assert!(!ingest.check_device_ingest_bytes("app", "device", 1).await);
        });
    }

    #[test]
    fn tiny_records_still_consume_item_budget() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let ingest = IngestSecurity::new();
            assert!(
                ingest
                    .check_device_ingest_items("app", "device", 1_000)
                    .await
            );
            assert!(
                ingest
                    .check_device_ingest_items("app", "device", 1_000)
                    .await
            );
            assert!(!ingest.check_device_ingest_items("app", "device", 1).await);
        });
    }

    #[test]
    fn second_factor_failures_throttle_further_attempts() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let security = AuthSecurity::new(b"pepper").unwrap();
            let keys = ["2fa-account:user-1", "2fa-source:203.0.113.7"];
            assert!(security.two_factor_gate(&keys).await.is_none());

            assert_eq!(security.record_two_factor_failure(&keys).await, 0);
            assert!(security.two_factor_gate(&keys).await.is_none());

            assert_eq!(security.record_two_factor_failure(&keys).await, 1);
            assert_eq!(security.two_factor_gate(&keys).await, Some(1));

            assert_eq!(security.record_two_factor_failure(&keys).await, 2);
            assert_eq!(security.two_factor_gate(&keys).await, Some(2));

            security.record_success(keys[0], keys[1]).await;
            assert!(security.two_factor_gate(&keys).await.is_none());
        });
    }

    #[test]
    fn pre_install_throttle_limits_each_source() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let throttle = PreInstallThrottle::new(2, 60_000);
            assert!(throttle.charge("203.0.113.7", 16).await);
            assert!(throttle.charge("203.0.113.7", 16).await);
            assert!(!throttle.charge("203.0.113.7", 16).await);
            assert!(throttle.charge("198.51.100.9", 16).await);
        });
    }

    #[test]
    fn pagination_bounds_are_clamped() {
        assert_eq!(bounded_page(Some(0)), 1);
        assert_eq!(bounded_page(None), 1);
        assert_eq!(bounded_page(Some(5)), 5);
        assert_eq!(bounded_page(Some(u64::MAX)), MAX_PAGE);
        assert_eq!(bounded_page_size(None), 50);
        assert_eq!(bounded_page_size(Some(0)), 1);
        assert_eq!(bounded_page_size(Some(10_000)), 200);
    }

    #[test]
    fn safe_identifier_accepts_valid_ids_and_rejects_traversal() {
        assert!(validate_safe_identifier("id", "06f2e1e2-6e89-4988-937c-d52967077522").is_ok());
        assert!(validate_safe_identifier("id", "production_v1.0").is_ok());
        assert!(validate_safe_identifier("id", "demo-app").is_ok());

        // Rejections:
        assert!(validate_safe_identifier("id", "").is_err());
        assert!(validate_safe_identifier("id", "   ").is_err());
        assert!(validate_safe_identifier("id", "../app").is_err());
        assert!(validate_safe_identifier("id", "..\\app").is_err());
        assert!(validate_safe_identifier("id", "app/sub").is_err());
        assert!(validate_safe_identifier("id", "app\\sub").is_err());
        assert!(validate_safe_identifier("id", "app\0bad").is_err());
        assert!(validate_safe_identifier("id", ".hidden").is_err());
        assert!(validate_safe_identifier("id", "-flag").is_err());
        assert!(validate_safe_identifier("id", "app space").is_err());
        assert!(validate_safe_identifier("id", "app:colon").is_err());
        assert!(validate_safe_identifier("id", &"a".repeat(129)).is_err());
    }

    #[test]
    fn safe_relative_path_blocks_traversal_and_encoding_tricks() {
        assert!(validate_safe_relative_path("assets/index.js").is_ok());
        assert!(validate_safe_relative_path("index.html").is_ok());
        assert!(validate_safe_relative_path("").is_ok());

        // Rejections:
        assert!(validate_safe_relative_path("../etc/passwd").is_err());
        assert!(validate_safe_relative_path("assets/../../secret").is_err());
        assert!(validate_safe_relative_path("assets\\secret").is_err());
        assert!(validate_safe_relative_path("assets/%2e%2e/secret").is_err());
        assert!(validate_safe_relative_path("assets/%252e%252e/secret").is_err());
        assert!(validate_safe_relative_path("assets/%00.js").is_err());
        assert!(validate_safe_relative_path("assets/\0.js").is_err());
    }
}
