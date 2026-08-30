use sha2::{Digest, Sha256};

use crate::{
    database::{ingest_auth_repo, telemetry_repo::TelemetryScope},
    domain::telemetry::{
        BatchReceipt, ErrorInput, EventInput, LogInput, MAX_BATCH_ITEMS, MetricInput, RejectedItem,
        ValidateTelemetry,
    },
    error::AppError,
    ingest_signature,
    state::InstalledState,
};

#[derive(Clone, Copy, Debug)]
pub struct IngestTokenContext<'a> {
    pub client_ip: &'a str,
    pub user_agent: &'a str,
    pub raw_key: Option<&'a str>,
}

#[derive(Clone, Copy, Debug)]
pub struct IngestRequestContext<'a> {
    pub client_ip: &'a str,
    pub user_agent: &'a str,
    pub credential: Option<&'a str>,
    pub signature: Option<&'a str>,
    pub timestamp: Option<&'a str>,
    pub nonce: Option<&'a str>,
    pub method: &'a str,
    pub path: &'a str,
}

#[derive(Clone, Debug)]
pub struct IngestScope {
    pub application_id: String,
    pub environment_id: String,
    source_ip: Option<String>,
    device_id: Option<String>,
    session_id: Option<String>,
    app_version: Option<String>,
    os: Option<String>,
}

impl IngestScope {
    fn storage_scope(&self) -> TelemetryScope {
        TelemetryScope {
            application_id: self.application_id.clone(),
            environment_id: self.environment_id.clone(),
        }
    }
}

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
    pub signature_version: &'static str,
    pub scopes: Vec<String>,
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

/// Exchange a long-lived API key for a short-lived device-bound ingest token.
pub async fn issue_token(
    installed: &InstalledState,
    request: IngestTokenContext<'_>,
    body: IngestTokenRequest,
) -> Result<IngestTokenResponse, AppError> {
    let user_agent = validate_user_agent(request.user_agent)?;

    if !installed
        .auth_security
        .ingest
        .check_ip_token_rate(request.client_ip)
        .await
    {
        return Err(AppError::TooManyRequests);
    }

    let device_id = validate_device_id(&body.device_id)?.to_owned();
    let session_id = normalize_optional_identifier(body.session_id, 128, "sessionId")?;
    let app_version = normalize_optional_text(body.app_version, 64, "appVersion")?;
    let os = normalize_optional_text(body.os, 128, "os")?;

    let raw_key = request.raw_key.ok_or(AppError::Unauthorized)?;
    let hash = hex::encode(Sha256::digest(raw_key.as_bytes()));
    let context = ingest_auth_repo::api_key_context(
        &installed.database,
        &hash,
        chrono::Utc::now().timestamp_millis(),
    )
    .await?
    .ok_or(AppError::Unauthorized)?;

    if !installed
        .auth_security
        .ingest
        .check_device_enrollment(request.client_ip, &context.application_id, &device_id)
        .await
        || !installed
            .auth_security
            .ingest
            .check_device_token_rate(&context.application_id, &device_id)
            .await
    {
        return Err(AppError::TooManyRequests);
    }

    let client_binding = installed
        .auth_security
        .ingest
        .client_binding(user_agent, installed.auth_security.pepper());
    let (token, signing_key, expires_at) = installed.auth_security.ingest.issue_ingest_token(
        &context.application_id,
        &context.environment_id,
        &device_id,
        session_id.as_deref(),
        app_version.as_deref(),
        os.as_deref(),
        &client_binding,
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
        signature_version: ingest_signature::SIGNATURE_VERSION,
        scopes: context.scopes,
    })
}

pub async fn scope_from_context_with_permission(
    installed: &InstalledState,
    request: IngestRequestContext<'_>,
    required_perm: &str,
    raw_body: &[u8],
) -> Result<IngestScope, AppError> {
    let user_agent = validate_user_agent(request.user_agent)?;

    if !installed
        .auth_security
        .ingest
        .check_ip_ingest_rate(request.client_ip)
        .await
        || !installed
            .auth_security
            .ingest
            .check_ip_ingest_bytes(request.client_ip, raw_body.len())
            .await
    {
        return Err(AppError::TooManyRequests);
    }

    let auth_header = request.credential.ok_or(AppError::Unauthorized)?;

    if auth_header.starts_with("sndt_") {
        let claims = installed
            .auth_security
            .ingest
            .verify_ingest_token(auth_header, installed.auth_security.pepper())
            .ok_or(AppError::Unauthorized)?;

        let expected_binding = installed
            .auth_security
            .ingest
            .client_binding(user_agent, installed.auth_security.pepper());
        if claims.client_binding != expected_binding {
            return Err(AppError::Forbidden);
        }

        if !has_permission(&claims.scopes, required_perm) {
            return Err(AppError::Forbidden);
        }

        let signature = request.signature.ok_or(AppError::Forbidden)?;
        let timestamp = request
            .timestamp
            .ok_or(AppError::Forbidden)?
            .parse::<i64>()
            .map_err(|_| AppError::Validation("invalid x-sonde-timestamp".into()))?;
        let nonce = request.nonce.ok_or(AppError::Forbidden)?;
        if nonce.len() < 8
            || nonce.len() > 128
            || nonce.chars().any(|value| value.is_control() || value.is_whitespace())
        {
            return Err(AppError::Validation(
                "x-sonde-nonce must be 8..128 visible non-whitespace bytes".into(),
            ));
        }

        let signing_key = installed
            .auth_security
            .ingest
            .signing_key_for_claims(&claims, installed.auth_security.pepper());
        ingest_signature::verify(
            &signing_key,
            timestamp,
            nonce,
            request.method,
            request.path,
            raw_body,
            signature,
        )?;

        let now = chrono::Utc::now().timestamp_millis();
        if !installed
            .auth_security
            .ingest
            .check_and_record_nonce(&claims.token_id, nonce, now + 120_000)
            .await
        {
            return Err(AppError::Forbidden);
        }

        if !installed
            .auth_security
            .ingest
            .check_device_rate(&claims.application_id, &claims.device_id)
            .await
            || !installed
                .auth_security
                .ingest
                .check_device_ingest_bytes(
                    &claims.application_id,
                    &claims.device_id,
                    raw_body.len(),
                )
                .await
        {
            return Err(AppError::TooManyRequests);
        }

        Ok(IngestScope {
            application_id: claims.application_id,
            environment_id: claims.environment_id,
            source_ip: Some(request.client_ip.to_owned()),
            device_id: Some(claims.device_id),
            session_id: claims.session_id,
            app_version: claims.app_version,
            os: claims.os,
        })
    } else {
        if !installed
            .auth_security
            .ingest
            .check_legacy_ingest_budget(request.client_ip, raw_body.len())
            .await
        {
            return Err(AppError::TooManyRequests);
        }
        let mut scope = scope_for_key_with_permission(installed, auth_header, required_perm).await?;
        scope.source_ip = Some(request.client_ip.to_owned());
        Ok(scope)
    }
}

pub async fn scope_from_context(
    installed: &InstalledState,
    request: IngestRequestContext<'_>,
    raw_body: &[u8],
) -> Result<IngestScope, AppError> {
    scope_from_context_with_permission(installed, request, "telemetry.ingest", raw_body).await
}

pub async fn scope_for_key_with_permission(
    installed: &InstalledState,
    raw_key: &str,
    required_perm: &str,
) -> Result<IngestScope, AppError> {
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

    Ok(IngestScope {
        application_id: context.application_id,
        environment_id: context.environment_id,
        source_ip: None,
        device_id: None,
        session_id: None,
        app_version: None,
        os: None,
    })
}

pub async fn scope_for_key(
    installed: &InstalledState,
    raw_key: &str,
) -> Result<IngestScope, AppError> {
    scope_for_key_with_permission(installed, raw_key, "telemetry.ingest").await
}

pub async fn events(
    installed: &InstalledState,
    scope: &IngestScope,
    mut items: Vec<EventInput>,
) -> Result<BatchReceipt, AppError> {
    ensure_batch_size(items.len())?;
    charge_item_budget(installed, scope, items.len()).await?;
    let (accepted, rejected) = validate_with(&mut items, |item| bind_event_dimensions(scope, item));
    let mut valid = select_valid(items, &rejected);
    let key_salt = format!("{}:{}", scope.application_id, scope.environment_id);
    for item in &mut valid {
        item.anonymous_id = item
            .anonymous_id
            .take()
            .map(|id| anonymous_hash(&id, &key_salt));
    }
    let storage_scope = scope.storage_scope();
    installed
        .ingest_writer
        .write_events(&storage_scope, valid)
        .await?;
    Ok(BatchReceipt { accepted, rejected })
}

pub async fn metrics(
    installed: &InstalledState,
    scope: &IngestScope,
    mut items: Vec<MetricInput>,
) -> Result<BatchReceipt, AppError> {
    ensure_batch_size(items.len())?;
    charge_item_budget(installed, scope, items.len()).await?;
    let (accepted, rejected) = validate_with(&mut items, |_| Ok(()));
    let valid = select_valid(items, &rejected);
    let storage_scope = scope.storage_scope();
    installed
        .ingest_writer
        .write_metrics(&storage_scope, valid)
        .await?;
    Ok(BatchReceipt { accepted, rejected })
}

pub async fn logs(
    installed: &InstalledState,
    scope: &IngestScope,
    mut items: Vec<LogInput>,
) -> Result<BatchReceipt, AppError> {
    ensure_batch_size(items.len())?;
    charge_item_budget(installed, scope, items.len()).await?;
    let (accepted, rejected) = validate_with(&mut items, |_| Ok(()));
    let valid = select_valid(items, &rejected);
    let storage_scope = scope.storage_scope();
    installed
        .ingest_writer
        .write_logs(&storage_scope, valid)
        .await?;
    Ok(BatchReceipt { accepted, rejected })
}

pub async fn errors(
    installed: &InstalledState,
    scope: &IngestScope,
    mut items: Vec<ErrorInput>,
) -> Result<BatchReceipt, AppError> {
    ensure_batch_size(items.len())?;
    charge_item_budget(installed, scope, items.len()).await?;
    let (accepted, rejected) = validate_with(&mut items, |item| bind_error_dimensions(scope, item));
    let valid = select_valid(items, &rejected);
    let storage_scope = scope.storage_scope();
    installed
        .ingest_writer
        .write_errors(&storage_scope, valid)
        .await?;
    Ok(BatchReceipt { accepted, rejected })
}

async fn charge_item_budget(
    installed: &InstalledState,
    scope: &IngestScope,
    item_count: usize,
) -> Result<(), AppError> {
    let Some(source_ip) = scope.source_ip.as_deref() else {
        return Ok(());
    };
    if !installed
        .auth_security
        .ingest
        .check_ip_ingest_items(source_ip, item_count)
        .await
    {
        return Err(AppError::TooManyRequests);
    }
    if let Some(device_id) = scope.device_id.as_deref() {
        if !installed
            .auth_security
            .ingest
            .check_device_ingest_items(&scope.application_id, device_id, item_count)
            .await
        {
            return Err(AppError::TooManyRequests);
        }
    } else if !installed
        .auth_security
        .ingest
        .check_legacy_ingest_items(source_ip, item_count)
        .await
    {
        return Err(AppError::TooManyRequests);
    }
    Ok(())
}

fn ensure_batch_size(length: usize) -> Result<(), AppError> {
    if length > MAX_BATCH_ITEMS {
        return Err(AppError::Validation(format!(
            "maximum batch size is {MAX_BATCH_ITEMS}"
        )));
    }
    Ok(())
}

fn validate_with<T: ValidateTelemetry>(
    items: &mut [T],
    mut bind: impl FnMut(&mut T) -> Result<(), &'static str>,
) -> (usize, Vec<RejectedItem>) {
    let mut accepted = 0;
    let mut rejected = Vec::new();
    for (index, item) in items.iter_mut().enumerate() {
        let result = bind(item).and_then(|()| item.validate());
        if let Err(reason) = result {
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

fn bind_event_dimensions(scope: &IngestScope, item: &mut EventInput) -> Result<(), &'static str> {
    bind_dimension(
        scope.device_id.as_deref(),
        &mut item.anonymous_id,
        "anonymous_id must match deviceId used to issue the ingest token",
    )?;
    bind_dimension(
        scope.session_id.as_deref(),
        &mut item.session_id,
        "session_id must match sessionId bound to the ingest token",
    )?;
    bind_dimension(
        scope.app_version.as_deref(),
        &mut item.app_version,
        "app_version must match appVersion bound to the ingest token",
    )?;
    bind_dimension(
        scope.os.as_deref(),
        &mut item.os,
        "os must match os bound to the ingest token",
    )
}

fn bind_error_dimensions(scope: &IngestScope, item: &mut ErrorInput) -> Result<(), &'static str> {
    bind_dimension(
        scope.device_id.as_deref(),
        &mut item.anonymous_id,
        "anonymous_id must match deviceId used to issue the ingest token",
    )?;
    bind_dimension(
        scope.session_id.as_deref(),
        &mut item.session_id,
        "session_id must match sessionId bound to the ingest token",
    )?;
    bind_dimension(
        scope.app_version.as_deref(),
        &mut item.app_version,
        "app_version must match appVersion bound to the ingest token",
    )?;
    bind_dimension(
        scope.os.as_deref(),
        &mut item.os,
        "os must match os bound to the ingest token",
    )
}

fn bind_dimension(
    bound: Option<&str>,
    value: &mut Option<String>,
    mismatch: &'static str,
) -> Result<(), &'static str> {
    let Some(bound) = bound else {
        return Ok(());
    };
    match value.as_deref() {
        Some(current) if current != bound => Err(mismatch),
        Some(_) => Ok(()),
        None => {
            *value = Some(bound.to_owned());
            Ok(())
        }
    }
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

fn validate_user_agent(value: &str) -> Result<&str, AppError> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 512
        || value.chars().any(|character| character.is_control())
    {
        return Err(AppError::Validation(
            "User-Agent must be 1..512 visible bytes".into(),
        ));
    }
    Ok(value)
}

fn validate_device_id(value: &str) -> Result<&str, AppError> {
    let value = value.trim();
    if value.len() < 4
        || value.len() > 128
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | ':')
        })
    {
        return Err(AppError::Validation(
            "deviceId must be 4..128 bytes using letters, digits, '-', '_', '.', or ':'".into(),
        ));
    }
    Ok(value)
}

fn normalize_optional_identifier(
    value: Option<String>,
    max_len: usize,
    field: &str,
) -> Result<Option<String>, AppError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty()
        || value.len() > max_len
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | ':')
        })
    {
        return Err(AppError::Validation(format!(
            "{field} must be 1..{max_len} bytes using a stable identifier format"
        )));
    }
    Ok(Some(value.to_owned()))
}

fn normalize_optional_text(
    value: Option<String>,
    max_len: usize,
    field: &str,
) -> Result<Option<String>, AppError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty()
        || value.len() > max_len
        || value.chars().any(|character| character.is_control())
    {
        return Err(AppError::Validation(format!(
            "{field} must be 1..{max_len} visible bytes"
        )));
    }
    Ok(Some(value.to_owned()))
}

fn anonymous_hash(value: &str, salt: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hasher.update(salt.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::{IngestScope, bind_event_dimensions};
    use crate::domain::telemetry::EventInput;

    #[test]
    fn signed_token_device_binding_blocks_identity_spoofing() {
        let scope = IngestScope {
            application_id: "app".into(),
            environment_id: "env".into(),
            source_ip: Some("127.0.0.1".into()),
            device_id: Some("device-1234".into()),
            session_id: Some("session-1".into()),
            app_version: Some("1.0.0".into()),
            os: Some("windows".into()),
        };
        let mut forged = EventInput {
            name: "startup".into(),
            timestamp: None,
            anonymous_id: Some("device-9999".into()),
            session_id: Some("session-1".into()),
            app_version: Some("1.0.0".into()),
            launcher_version: None,
            os: Some("windows".into()),
            idempotency_key: None,
            attributes: Default::default(),
        };
        assert!(bind_event_dimensions(&scope, &mut forged).is_err());

        let mut missing = EventInput {
            name: "startup".into(),
            timestamp: None,
            anonymous_id: None,
            session_id: None,
            app_version: None,
            launcher_version: None,
            os: None,
            idempotency_key: None,
            attributes: Default::default(),
        };
        assert!(bind_event_dimensions(&scope, &mut missing).is_ok());
        assert_eq!(missing.anonymous_id.as_deref(), Some("device-1234"));
        assert_eq!(missing.session_id.as_deref(), Some("session-1"));
        assert_eq!(missing.app_version.as_deref(), Some("1.0.0"));
        assert_eq!(missing.os.as_deref(), Some("windows"));
    }
}
