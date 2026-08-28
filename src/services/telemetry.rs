use actix_web::HttpRequest;
use sha2::{Digest, Sha256};

use crate::{
    auth,
    database::{ingest_auth_repo, telemetry_repo::TelemetryScope},
    domain::telemetry::{
        BatchReceipt, ErrorInput, EventInput, LogInput, MAX_BATCH_ITEMS, MetricInput, RejectedItem,
        ValidateTelemetry,
    },
    error::AppError,
    security::IngestSecurity,
    state::InstalledState,
};

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestTokenRequest {
    pub device_id: String,
    pub session_id: Option<String>,
    pub app_version: Option<String>,
    pub os: Option<String>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestTokenResponse {
    pub token: String,
    pub token_type: &'static str,
    pub expires_in: i64,
    pub expires_at: i64,
    pub signing_key: String,
    pub scopes: Vec<String>,
}

/// Return the transport peer address used by Actix.
///
/// Forwarded/X-Forwarded-For are deliberately not trusted by default: accepting them without a
/// configured trusted-proxy boundary would allow a direct client to bypass IP based throttling by
/// spoofing request headers. Reverse proxies should therefore enforce rate limits themselves until
/// Sonde grows an explicit trusted-proxy configuration.
pub fn extract_client_ip(request: &HttpRequest) -> String {
    request
        .peer_addr()
        .map(|address| address.ip().to_string())
        .unwrap_or_else(|| "unknown".into())
}

pub fn extract_raw_key(request: &HttpRequest) -> Option<&str> {
    auth::bearer_token(request)
        .or_else(|| {
            request
                .headers()
                .get("x-sonde-token")
                .and_then(|value| value.to_str().ok())
        })
        .or_else(|| {
            request
                .headers()
                .get("x-sonde-key")
                .and_then(|value| value.to_str().ok())
        })
        .or_else(|| {
            request
                .headers()
                .get("x-api-key")
                .and_then(|value| value.to_str().ok())
        })
}

pub fn has_permission(scopes: &[String], required_perm: &str) -> bool {
    scopes.iter().any(|scope| {
        scope == "*"
            || scope == "ingest"
            || scope == "telemetry.ingest"
            || scope == required_perm
            || (required_perm == "telemetry.events"
                && (scope == "events" || scope == "ingest.events"))
            || (required_perm == "telemetry.metrics"
                && (scope == "metrics" || scope == "ingest.metrics"))
            || (required_perm == "telemetry.logs"
                && (scope == "logs" || scope == "ingest.logs"))
            || (required_perm == "telemetry.errors"
                && (scope == "errors" || scope == "ingest.errors"))
    })
}

pub fn match_user_agent(rule: &str, user_agent: &str) -> bool {
    let rule = rule.trim();
    if rule.is_empty() || rule == "*" {
        return true;
    }
    if user_agent.is_empty() {
        return false;
    }
    for part in rule.split(',') {
        let pattern = part.trim();
        if pattern.is_empty() {
            continue;
        }
        if let Some(prefix) = pattern.strip_suffix('*') {
            if user_agent.starts_with(prefix) {
                return true;
            }
        } else if user_agent == pattern || user_agent.contains(pattern) {
            return true;
        }
    }
    false
}

/// Exchange API Key + Device ID for an ephemeral 2-minute Ingest Token.
pub async fn issue_token_from_request(
    installed: &InstalledState,
    request: &HttpRequest,
    body: IngestTokenRequest,
) -> Result<IngestTokenResponse, AppError> {
    let user_agent = request
        .headers()
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .trim();

    if user_agent.is_empty() {
        return Err(AppError::Validation(
            "valid User-Agent header is required to request ingest token".into(),
        ));
    }

    let client_ip = extract_client_ip(request);
    if !installed
        .auth_security
        .ingest
        .check_ip_token_rate(&client_ip)
        .await
    {
        return Err(AppError::TooManyRequests);
    }

    let device_id = body.device_id.trim();
    if device_id.is_empty() || device_id.len() > 256 {
        return Err(AppError::Validation(
            "deviceId must be 1..256 bytes".into(),
        ));
    }

    let raw_key = extract_raw_key(request).ok_or(AppError::Unauthorized)?;
    let hash = hex::encode(Sha256::digest(raw_key.as_bytes()));
    let context = ingest_auth_repo::api_key_context(
        &installed.database,
        &hash,
        chrono::Utc::now().timestamp_millis(),
    )
    .await?
    .ok_or(AppError::Unauthorized)?;

    let (token, signing_key, expires_at) = installed.auth_security.ingest.issue_ingest_token(
        &context.application_id,
        &context.environment_id,
        device_id,
        &context.scopes,
        120,
        installed.auth_security.pepper(),
    )?;

    Ok(IngestTokenResponse {
        token,
        token_type: "Bearer",
        expires_in: 120,
        expires_at,
        signing_key,
        scopes: context.scopes,
    })
}

pub async fn scope_from_request_with_permission(
    installed: &InstalledState,
    request: &HttpRequest,
    required_perm: &str,
    raw_body: &[u8],
) -> Result<TelemetryScope, AppError> {
    let user_agent = request
        .headers()
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .trim();

    if user_agent.is_empty() {
        return Err(AppError::Validation(
            "valid User-Agent header is required for telemetry ingestion".into(),
        ));
    }

    let client_ip = extract_client_ip(request);
    if !installed
        .auth_security
        .ingest
        .check_ip_ingest_rate(&client_ip)
        .await
    {
        return Err(AppError::TooManyRequests);
    }

    let auth_header = extract_raw_key(request).ok_or(AppError::Unauthorized)?;

    if auth_header.starts_with("sndt_") {
        let claims = installed
            .auth_security
            .ingest
            .verify_ingest_token(auth_header, installed.auth_security.pepper())
            .ok_or(AppError::Unauthorized)?;

        if !has_permission(&claims.scopes, required_perm) {
            return Err(AppError::Forbidden);
        }

        let signature = required_header(request, "x-sonde-signature")?;
        let timestamp = required_header(request, "x-sonde-timestamp")?
            .parse::<i64>()
            .map_err(|_| AppError::Validation("invalid x-sonde-timestamp".into()))?;
        let nonce = required_header(request, "x-sonde-nonce")?;
        if nonce.is_empty() || nonce.len() > 128 {
            return Err(AppError::Validation(
                "x-sonde-nonce must be 1..128 bytes".into(),
            ));
        }

        let signing_key = installed
            .auth_security
            .ingest
            .signing_key_for_claims(&claims, installed.auth_security.pepper());
        IngestSecurity::verify_request_signature(
            &signing_key,
            timestamp,
            nonce,
            raw_body,
            signature,
        )?;

        let now = chrono::Utc::now().timestamp_millis();
        if !installed
            .auth_security
            .ingest
            .check_and_record_nonce(&claims.application_id, nonce, now + 120_000)
            .await
        {
            return Err(AppError::Forbidden);
        }

        if !installed
            .auth_security
            .ingest
            .check_device_rate(&claims.application_id, &claims.device_id)
            .await
        {
            return Err(AppError::TooManyRequests);
        }

        Ok(TelemetryScope {
            application_id: claims.application_id,
            environment_id: claims.environment_id,
        })
    } else {
        scope_for_key_with_permission(installed, auth_header, required_perm).await
    }
}

pub async fn scope_from_request(
    installed: &InstalledState,
    request: &HttpRequest,
    raw_body: &[u8],
) -> Result<TelemetryScope, AppError> {
    scope_from_request_with_permission(installed, request, "telemetry.ingest", raw_body).await
}

fn required_header<'a>(request: &'a HttpRequest, name: &str) -> Result<&'a str, AppError> {
    request
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .ok_or(AppError::Forbidden)
}

pub async fn scope_for_key_with_permission(
    installed: &InstalledState,
    raw_key: &str,
    required_perm: &str,
) -> Result<TelemetryScope, AppError> {
    let hash = hex::encode(Sha256::digest(raw_key.as_bytes()));
    let context = ingest_auth_repo::api_key_context(
        &installed.database,
        &hash,
        chrono::Utc::now().timestamp_millis(),
    )
    .await?
    .ok_or(AppError::Unauthorized)?;

    if !has_permission(&context.scopes, required_perm) {
        return Err(AppError::Forbidden);
    }

    Ok(TelemetryScope {
        application_id: context.application_id,
        environment_id: context.environment_id,
    })
}

pub async fn scope_for_key(
    installed: &InstalledState,
    raw_key: &str,
) -> Result<TelemetryScope, AppError> {
    scope_for_key_with_permission(installed, raw_key, "telemetry.ingest").await
}

pub async fn events(
    installed: &InstalledState,
    scope: &TelemetryScope,
    mut items: Vec<EventInput>,
) -> Result<BatchReceipt, AppError> {
    ensure_batch_size(items.len())?;
    let (accepted, rejected) = validate(items.as_slice());
    let key_salt = format!("{}:{}", scope.application_id, scope.environment_id);
    for item in &mut items {
        item.anonymous_id = item
            .anonymous_id
            .take()
            .map(|id| anonymous_hash(&id, &key_salt));
    }
    let valid = select_valid(items, &rejected);
    installed.ingest_writer.write_events(scope, valid).await?;
    Ok(BatchReceipt { accepted, rejected })
}

pub async fn metrics(
    installed: &InstalledState,
    scope: &TelemetryScope,
    items: Vec<MetricInput>,
) -> Result<BatchReceipt, AppError> {
    ensure_batch_size(items.len())?;
    let (accepted, rejected) = validate(items.as_slice());
    let valid = select_valid(items, &rejected);
    installed.ingest_writer.write_metrics(scope, valid).await?;
    Ok(BatchReceipt { accepted, rejected })
}

pub async fn logs(
    installed: &InstalledState,
    scope: &TelemetryScope,
    items: Vec<LogInput>,
) -> Result<BatchReceipt, AppError> {
    ensure_batch_size(items.len())?;
    let (accepted, rejected) = validate(items.as_slice());
    let valid = select_valid(items, &rejected);
    installed.ingest_writer.write_logs(scope, valid).await?;
    Ok(BatchReceipt { accepted, rejected })
}

pub async fn errors(
    installed: &InstalledState,
    scope: &TelemetryScope,
    items: Vec<ErrorInput>,
) -> Result<BatchReceipt, AppError> {
    ensure_batch_size(items.len())?;
    let (accepted, rejected) = validate(items.as_slice());
    let valid = select_valid(items, &rejected);
    installed.ingest_writer.write_errors(scope, valid).await?;
    Ok(BatchReceipt { accepted, rejected })
}

fn ensure_batch_size(length: usize) -> Result<(), AppError> {
    if length > MAX_BATCH_ITEMS {
        return Err(AppError::Validation(format!(
            "maximum batch size is {MAX_BATCH_ITEMS}"
        )));
    }
    Ok(())
}

fn validate<T: ValidateTelemetry>(items: &[T]) -> (usize, Vec<RejectedItem>) {
    let mut accepted = 0;
    let mut rejected = Vec::new();
    for (index, item) in items.iter().enumerate() {
        if let Err(reason) = item.validate() {
            rejected.push(RejectedItem {
                index,
                reason: reason.to_string(),
            });
        } else {
            accepted += 1;
        }
    }
    (accepted, rejected)
}

fn select_valid<T>(items: Vec<T>, rejected: &[RejectedItem]) -> Vec<T> {
    if rejected.is_empty() {
        return items;
    }
    let rejected_indices: std::collections::HashSet<_> =
        rejected.iter().map(|item| item.index).collect();
    items
        .into_iter()
        .enumerate()
        .filter_map(|(index, item)| {
            if rejected_indices.contains(&index) {
                None
            } else {
                Some(item)
            }
        })
        .collect()
}

fn anonymous_hash(value: &str, salt: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hasher.update(salt.as_bytes());
    hex::encode(hasher.finalize())
}
