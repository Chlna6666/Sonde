use std::collections::HashMap;
use std::sync::Arc;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::Sha256;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

use crate::{auth, error::AppError};

const SESSION_ABSOLUTE_MILLIS: i64 = 8 * 60 * 60 * 1_000;
const SESSION_IDLE_MILLIS: i64 = 30 * 60 * 1_000;
const CHALLENGE_MILLIS: i64 = 5 * 60 * 1_000;
const INGEST_SIGNING_CONTEXT: &[u8] = b"sonde-ingest-signing-v1\n";

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone, Debug)]
pub struct SessionSnapshot {
    pub user_id: String,
    pub csrf_token: String,
}

#[derive(Debug)]
struct MemorySession {
    user_id: String,
    csrf_token: String,
    expires_at: i64,
    last_seen_at: i64,
}

#[derive(Debug, Default)]
struct Attempt {
    failures: u32,
    next_allowed_at: i64,
}

#[derive(Debug)]
struct Challenge {
    account_key: String,
    source_key: String,
    answer_hash: String,
    expires_at: i64,
}

#[derive(Debug)]
pub enum LoginGate {
    Allowed,
    ChallengeRequired {
        challenge_id: String,
        prompt: String,
    },
    Delayed {
        retry_after_seconds: u64,
    },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct IngestTokenClaims {
    pub application_id: String,
    pub environment_id: String,
    pub device_id: String,
    pub scopes: Vec<String>,
    pub expires_at: i64,
    pub token_id: String,
}

#[derive(Debug)]
struct RateBucket {
    count: u32,
    window_start: i64,
}

impl RateBucket {
    fn new(now: i64) -> Self {
        Self {
            count: 1,
            window_start: now,
        }
    }

    fn check_and_increment(&mut self, now: i64, max_requests: u32, window_ms: i64) -> bool {
        if now - self.window_start >= window_ms {
            self.count = 1;
            self.window_start = now;
            true
        } else if self.count < max_requests {
            self.count += 1;
            true
        } else {
            false
        }
    }
}

pub struct IngestSecurity {
    ip_token_rate: Mutex<HashMap<String, RateBucket>>,
    ip_ingest_rate: Mutex<HashMap<String, RateBucket>>,
    device_rate: Mutex<HashMap<String, RateBucket>>,
    seen_nonces: Mutex<HashMap<String, i64>>,
}

impl Default for IngestSecurity {
    fn default() -> Self {
        Self::new()
    }
}

impl IngestSecurity {
    pub fn new() -> Self {
        Self {
            ip_token_rate: Mutex::new(HashMap::new()),
            ip_ingest_rate: Mutex::new(HashMap::new()),
            device_rate: Mutex::new(HashMap::new()),
            seen_nonces: Mutex::new(HashMap::new()),
        }
    }

    /// IP rate limit for Token exchange (max 30 requests / 60 seconds)
    pub async fn check_ip_token_rate(&self, ip: &str) -> bool {
        let now = chrono::Utc::now().timestamp_millis();
        let mut buckets = self.ip_token_rate.lock().await;
        if buckets.len() > 20_000 {
            buckets.retain(|_, b| now - b.window_start < 60_000);
        }
        buckets
            .entry(ip.to_owned())
            .or_insert_with(|| RateBucket::new(now))
            .check_and_increment(now, 30, 60_000)
    }

    /// IP rate limit for Ingestion endpoints (max 120 requests / 60 seconds)
    pub async fn check_ip_ingest_rate(&self, ip: &str) -> bool {
        let now = chrono::Utc::now().timestamp_millis();
        let mut buckets = self.ip_ingest_rate.lock().await;
        if buckets.len() > 20_000 {
            buckets.retain(|_, b| now - b.window_start < 60_000);
        }
        buckets
            .entry(ip.to_owned())
            .or_insert_with(|| RateBucket::new(now))
            .check_and_increment(now, 120, 60_000)
    }

    /// Device ID rate limit (max 60 requests / 60 seconds per device)
    pub async fn check_device_rate(&self, app_id: &str, device_id: &str) -> bool {
        let now = chrono::Utc::now().timestamp_millis();
        let key = format!("{}:{}", app_id, device_id);
        let mut buckets = self.device_rate.lock().await;
        if buckets.len() > 50_000 {
            buckets.retain(|_, b| now - b.window_start < 60_000);
        }
        buckets
            .entry(key)
            .or_insert_with(|| RateBucket::new(now))
            .check_and_increment(now, 60, 60_000)
    }

    /// Anti-replay Nonce validation (deduplicates nonces in a 120-second sliding window)
    pub async fn check_and_record_nonce(&self, app_id: &str, nonce: &str, expires_at: i64) -> bool {
        let now = chrono::Utc::now().timestamp_millis();
        let key = format!("{}:{}", app_id, nonce);
        let mut nonces = self.seen_nonces.lock().await;
        if nonces.len() > 50_000 {
            nonces.retain(|_, exp| *exp > now);
        }
        if let Some(exp) = nonces.get(&key) {
            if *exp > now {
                return false;
            }
        }
        nonces.insert(key, expires_at);
        true
    }

    /// Issue an ephemeral Ingest Token. The request signing key is returned separately and is not
    /// embedded in the token claims.
    pub fn issue_ingest_token(
        &self,
        application_id: &str,
        environment_id: &str,
        device_id: &str,
        scopes: &[String],
        ttl_seconds: i64,
        pepper: &[u8],
    ) -> Result<(String, String, i64), AppError> {
        let now = chrono::Utc::now().timestamp_millis();
        let expires_at = now + ttl_seconds * 1_000;
        let token_id = auth::random_token(18);
        let signing_key = derive_ingest_signing_key(pepper, &token_id);

        let claims = IngestTokenClaims {
            application_id: application_id.to_owned(),
            environment_id: environment_id.to_owned(),
            device_id: device_id.to_owned(),
            scopes: scopes.to_vec(),
            expires_at,
            token_id,
        };

        let json_bytes = serde_json::to_vec(&claims).map_err(|_| AppError::Internal)?;
        let payload_b64 = URL_SAFE_NO_PAD.encode(json_bytes);
        let sig = hmac_sha256(pepper, payload_b64.as_bytes());
        let token = format!("sndt_{}.{}", payload_b64, hex::encode(sig));

        Ok((token, signing_key, expires_at))
    }

    /// Verify an ephemeral Ingest Token.
    pub fn verify_ingest_token(
        &self,
        token_str: &str,
        pepper: &[u8],
    ) -> Option<IngestTokenClaims> {
        let without_prefix = token_str.strip_prefix("sndt_")?;
        let (payload_b64, sig_hex) = without_prefix.split_once('.')?;
        let provided_sig = hex::decode(sig_hex).ok()?;
        if !verify_hmac_sha256(pepper, payload_b64.as_bytes(), &provided_sig) {
            return None;
        }

        let json_bytes = URL_SAFE_NO_PAD.decode(payload_b64).ok()?;
        let claims: IngestTokenClaims = serde_json::from_slice(&json_bytes).ok()?;

        let now = chrono::Utc::now().timestamp_millis();
        if claims.expires_at <= now || claims.token_id.is_empty() {
            return None;
        }

        Some(claims)
    }

    pub fn signing_key_for_claims(&self, claims: &IngestTokenClaims, pepper: &[u8]) -> String {
        derive_ingest_signing_key(pepper, &claims.token_id)
    }

    /// Verify HMAC-SHA256 Request Signature & Timestamp Drift.
    pub fn verify_request_signature(
        signing_key: &str,
        timestamp_ms: i64,
        nonce: &str,
        body: &[u8],
        provided_sig_hex: &str,
    ) -> Result<(), AppError> {
        let now = chrono::Utc::now().timestamp_millis();
        if (now - timestamp_ms).abs() > 60_000 {
            return Err(AppError::Validation(
                "request timestamp drift too large (allowed +/- 60s)".into(),
            ));
        }

        let mut data_to_sign = Vec::with_capacity(64 + body.len());
        data_to_sign.extend_from_slice(timestamp_ms.to_string().as_bytes());
        data_to_sign.push(b'\n');
        data_to_sign.extend_from_slice(nonce.as_bytes());
        data_to_sign.push(b'\n');
        data_to_sign.extend_from_slice(body);

        let provided_sig = hex::decode(provided_sig_hex).map_err(|_| AppError::Forbidden)?;
        if !verify_hmac_sha256(signing_key.as_bytes(), &data_to_sign, &provided_sig) {
            return Err(AppError::Forbidden);
        }

        Ok(())
    }
}

fn derive_ingest_signing_key(pepper: &[u8], token_id: &str) -> String {
    let mut context = Vec::with_capacity(INGEST_SIGNING_CONTEXT.len() + token_id.len());
    context.extend_from_slice(INGEST_SIGNING_CONTEXT);
    context.extend_from_slice(token_id.as_bytes());
    format!("sec_{}", URL_SAFE_NO_PAD.encode(hmac_sha256(pepper, &context)))
}

fn verify_hmac_sha256(key: &[u8], data: &[u8], provided: &[u8]) -> bool {
    let Ok(mut mac) = HmacSha256::new_from_slice(key) else {
        return false;
    };
    mac.update(data);
    mac.verify_slice(provided).is_ok()
}

pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let Ok(mut mac) = HmacSha256::new_from_slice(key) else {
        return [0_u8; 32];
    };
    mac.update(data);
    let bytes = mac.finalize().into_bytes();
    let mut output = [0_u8; 32];
    output.copy_from_slice(&bytes);
    output
}

#[derive(Debug)]
struct TwoFactorPending {
    user_id: String,
    expires_at: i64,
}

pub struct AuthSecurity {
    sessions: RwLock<HashMap<String, MemorySession>>,
    attempts: Mutex<HashMap<String, Attempt>>,
    challenges: Mutex<HashMap<String, Challenge>>,
    two_factor_tokens: Mutex<HashMap<String, TwoFactorPending>>,
    used_totp_steps: Mutex<HashMap<String, u64>>,
    pub ingest: Arc<IngestSecurity>,
    pepper: Vec<u8>,
    dummy_password_hash: String,
}

impl AuthSecurity {
    pub fn new(pepper: &[u8]) -> Result<Self, AppError> {
        Ok(Self {
            sessions: RwLock::new(HashMap::new()),
            attempts: Mutex::new(HashMap::new()),
            challenges: Mutex::new(HashMap::new()),
            two_factor_tokens: Mutex::new(HashMap::new()),
            used_totp_steps: Mutex::new(HashMap::new()),
            ingest: Arc::new(IngestSecurity::new()),
            pepper: pepper.to_vec(),
            dummy_password_hash: auth::hash_password(
                "sonde-dummy-credential-never-used",
                pepper,
            )?,
        })
    }

    pub fn pepper(&self) -> &[u8] {
        &self.pepper
    }

    pub fn dummy_password_hash(&self) -> &str {
        &self.dummy_password_hash
    }

    pub async fn issue_2fa_temp_token(&self, user_id: &str) -> String {
        let token = format!("2fa_{}_{}", Uuid::now_v7(), auth::random_token(16));
        let now = chrono::Utc::now().timestamp_millis();
        let mut tokens = self.two_factor_tokens.lock().await;
        if tokens.len() > 10_000 {
            tokens.retain(|_, v| v.expires_at > now);
        }
        tokens.insert(
            token.clone(),
            TwoFactorPending {
                user_id: user_id.to_owned(),
                expires_at: now + 5 * 60 * 1_000,
            },
        );
        token
    }

    pub async fn consume_2fa_temp_token(&self, token: &str) -> Option<String> {
        let now = chrono::Utc::now().timestamp_millis();
        let mut tokens = self.two_factor_tokens.lock().await;
        let entry = tokens.remove(token)?;
        if entry.expires_at > now {
            Some(entry.user_id)
        } else {
            None
        }
    }

    pub async fn verify_and_consume_totp(&self, user_id: &str, secret: &str, code: &str) -> bool {
        let Some(step) = crate::totp::verify_totp_step(secret, code) else {
            return false;
        };

        let mut used_steps = self.used_totp_steps.lock().await;
        let now_sec = chrono::Utc::now().timestamp() as u64;
        if used_steps.len() > 20_000 {
            let oldest_allowed_step = (now_sec / 30).saturating_sub(10);
            used_steps.retain(|_, last_step| *last_step >= oldest_allowed_step);
        }

        if let Some(&last_step) = used_steps.get(user_id)
            && step <= last_step
        {
            return false;
        }

        used_steps.insert(user_id.to_owned(), step);
        true
    }

    pub async fn create_session(&self, user_id: &str) -> (String, String) {
        let token = auth::random_token(32);
        let csrf_token = auth::random_token(24);
        let now = chrono::Utc::now().timestamp_millis();
        let mut sessions = self.sessions.write().await;
        if sessions.len() > 10_000 {
            sessions.retain(|_, s| {
                s.expires_at > now && s.last_seen_at + SESSION_IDLE_MILLIS > now
            });
        }
        sessions.insert(
            auth::token_hash(&token),
            MemorySession {
                user_id: user_id.to_owned(),
                csrf_token: csrf_token.clone(),
                expires_at: now + SESSION_ABSOLUTE_MILLIS,
                last_seen_at: now,
            },
        );
        (token, csrf_token)
    }

    pub async fn session(&self, token_hash: &str) -> Option<SessionSnapshot> {
        let now = chrono::Utc::now().timestamp_millis();
        let mut sessions = self.sessions.write().await;
        let session = sessions.get_mut(token_hash)?;
        if session.expires_at <= now || session.last_seen_at + SESSION_IDLE_MILLIS <= now {
            sessions.remove(token_hash);
            return None;
        }
        session.last_seen_at = now;
        Some(SessionSnapshot {
            user_id: session.user_id.clone(),
            csrf_token: session.csrf_token.clone(),
        })
    }

    pub async fn revoke_session(&self, token_hash: &str) {
        self.sessions.write().await.remove(token_hash);
    }

    pub async fn login_gate(
        &self,
        account_key: &str,
        source_key: &str,
        challenge_id: Option<&str>,
        challenge_response: Option<&str>,
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
            .consume_challenge(
                account_key,
                source_key,
                challenge_id,
                challenge_response,
                now,
            )
            .await
        {
            LoginGate::Allowed
        } else {
            self.issue_challenge(account_key, source_key, now).await
        }
    }

    pub async fn record_failure(&self, account_key: &str, source_key: &str) -> LoginGate {
        let now = chrono::Utc::now().timestamp_millis();
        let mut attempts = self.attempts.lock().await;
        if attempts.len() > 20_000 {
            attempts.retain(|_, a| a.next_allowed_at > now - 86_400_000);
        }
        for key in [account_key, source_key] {
            let attempt = attempts.entry(key.to_owned()).or_default();
            attempt.failures = attempt.failures.saturating_add(1);
            let delay_seconds = attempt
                .failures
                .checked_sub(2)
                .map_or(0, |power| 2_u64.saturating_pow(power).min(300));
            attempt.next_allowed_at = now + delay_seconds as i64 * 1_000;
        }
        drop(attempts);
        self.issue_challenge(account_key, source_key, now).await
    }

    pub async fn record_success(&self, account_key: &str, source_key: &str) {
        let mut attempts = self.attempts.lock().await;
        attempts.remove(account_key);
        attempts.remove(source_key);
    }

    async fn issue_challenge(&self, account_key: &str, source_key: &str, now: i64) -> LoginGate {
        let challenge_id = auth::random_token(18);
        let prompt = challenge_code();
        let answer_hash = auth::token_hash(&prompt);
        let mut challenges = self.challenges.lock().await;
        if challenges.len() > 20_000 {
            challenges.retain(|_, c| c.expires_at > now);
        }
        challenges.insert(
            challenge_id.clone(),
            Challenge {
                account_key: account_key.to_owned(),
                source_key: source_key.to_owned(),
                answer_hash,
                expires_at: now + CHALLENGE_MILLIS,
            },
        );
        LoginGate::ChallengeRequired {
            challenge_id,
            prompt,
        }
    }

    async fn consume_challenge(
        &self,
        account_key: &str,
        source_key: &str,
        challenge_id: Option<&str>,
        challenge_response: Option<&str>,
        now: i64,
    ) -> bool {
        let (Some(challenge_id), Some(challenge_response)) = (challenge_id, challenge_response)
        else {
            return false;
        };
        let mut challenges = self.challenges.lock().await;
        let Some(challenge) = challenges.remove(challenge_id) else {
            return false;
        };
        challenge.expires_at > now
            && challenge.account_key == account_key
            && challenge.source_key == source_key
            && challenge.answer_hash == auth::token_hash(challenge_response.trim())
    }
}

fn challenge_code() -> String {
    let mut bytes = [0_u8; 3];
    rand::rng().fill_bytes(&mut bytes);
    let value = u32::from_be_bytes([0, bytes[0], bytes[1], bytes[2]]) % 1_000_000;
    format!("{value:06}")
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
pub mod tests {
    use super::*;

    #[test]
    fn sessions_are_process_local_and_revocable() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let security = AuthSecurity::new(b"pepper").unwrap();
            let (token, csrf) = security.create_session("usr_1").await;
            let snapshot = security.session(&auth::token_hash(&token)).await.unwrap();
            assert_eq!(snapshot.user_id, "usr_1");
            assert_eq!(snapshot.csrf_token, csrf);
            security.revoke_session(&auth::token_hash(&token)).await;
            assert!(security.session(&auth::token_hash(&token)).await.is_none());
        });
    }

    #[test]
    fn first_failure_requires_a_one_time_challenge() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let security = AuthSecurity::new(b"pepper").unwrap();
            assert!(matches!(
                security.login_gate("admin", "127.0.0.1", None, None).await,
                LoginGate::Allowed
            ));

            let challenge = security.record_failure("admin", "127.0.0.1").await;
            let LoginGate::ChallengeRequired {
                challenge_id,
                prompt,
            } = challenge
            else {
                panic!("expected challenge");
            };

            assert!(matches!(
                security
                    .login_gate("admin", "127.0.0.1", Some(&challenge_id), Some(&prompt))
                    .await,
                LoginGate::Allowed
            ));
        });
    }

    #[test]
    fn test_hmac_and_ingest_token_and_anti_replay() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let pepper = b"test-secret-pepper-32-bytes-long!";
            let ingest = IngestSecurity::new();

            let (token, signing_key, expires_at) = ingest
                .issue_ingest_token(
                    "app-uuid-1",
                    "env-uuid-1",
                    "device-12345",
                    &["telemetry.events".into(), "telemetry.errors".into()],
                    120,
                    pepper,
                )
                .unwrap();

            assert!(token.starts_with("sndt_"));
            let claims = ingest.verify_ingest_token(&token, pepper).unwrap();
            assert_eq!(claims.application_id, "app-uuid-1");
            assert_eq!(claims.device_id, "device-12345");
            assert_eq!(ingest.signing_key_for_claims(&claims, pepper), signing_key);
            assert_eq!(claims.expires_at, expires_at);
            assert!(claims.scopes.contains(&"telemetry.events".to_string()));

            let payload_b64 = token
                .strip_prefix("sndt_")
                .and_then(|value| value.split_once('.'))
                .map(|(payload, _)| payload)
                .unwrap();
            let decoded = String::from_utf8(URL_SAFE_NO_PAD.decode(payload_b64).unwrap()).unwrap();
            assert!(!decoded.contains(&signing_key));

            let now = chrono::Utc::now().timestamp_millis();
            assert!(
                ingest
                    .check_and_record_nonce("app-uuid-1", "nonce-abc", now + 120_000)
                    .await
            );
            assert!(
                !ingest
                    .check_and_record_nonce("app-uuid-1", "nonce-abc", now + 120_000)
                    .await
            );

            let body = b"{\"items\":[]}";
            let mut data_to_sign = Vec::new();
            data_to_sign.extend_from_slice(now.to_string().as_bytes());
            data_to_sign.push(b'\n');
            data_to_sign.extend_from_slice(b"nonce-xyz");
            data_to_sign.push(b'\n');
            data_to_sign.extend_from_slice(body);

            let sig = hmac_sha256(signing_key.as_bytes(), &data_to_sign);
            let sig_hex = hex::encode(sig);

            assert!(
                IngestSecurity::verify_request_signature(
                    &signing_key,
                    now,
                    "nonce-xyz",
                    body,
                    &sig_hex,
                )
                .is_ok()
            );
            assert!(
                IngestSecurity::verify_request_signature(
                    &signing_key,
                    now,
                    "nonce-xyz",
                    b"tampered",
                    &sig_hex,
                )
                .is_err()
            );
        });
    }

    #[test]
    fn test_totp_anti_replay_and_temp_tokens() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let security = AuthSecurity::new(b"pepper").unwrap();
            let secret = crate::totp::generate_totp_secret();
            let now_step = (chrono::Utc::now().timestamp() as u64) / 30;
            let code = crate::totp::compute_totp(&secret, now_step).unwrap();

            assert!(
                security
                    .verify_and_consume_totp("user-1", &secret, &code)
                    .await
            );
            assert!(
                !security
                    .verify_and_consume_totp("user-1", &secret, &code)
                    .await
            );

            let past_code = crate::totp::compute_totp(&secret, now_step - 1).unwrap();
            assert!(
                !security
                    .verify_and_consume_totp("user-1", &secret, &past_code)
                    .await
            );

            let temp_token = security.issue_2fa_temp_token("user-2").await;
            assert_eq!(
                security.consume_2fa_temp_token(&temp_token).await,
                Some("user-2".into())
            );
            assert_eq!(security.consume_2fa_temp_token(&temp_token).await, None);
        });
    }
}
