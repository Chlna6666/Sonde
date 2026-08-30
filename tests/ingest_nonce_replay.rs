use std::sync::Arc;

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use sonde::{
    config::InstallationConfig,
    database::{self, ingest_nonce_repo},
    error::AppError,
    security::AuthSecurity,
    services::telemetry::{self, IngestRequestContext},
    state::InstalledState,
};

type HmacSha256 = Hmac<Sha256>;

#[tokio::test]
async fn nonce_replay_is_rejected_across_independent_database_connections(
) -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("nonce-replay.sqlite");
    let database_url = format!(
        "sqlite://{}?mode=rwc",
        path.to_string_lossy().replace('\\', "/")
    );

    let replica_a = database::connect(&database_url).await?;
    database::migrate(&replica_a).await?;
    let replica_b = database::connect(&database_url).await?;

    let now = chrono::Utc::now().timestamp_millis();
    let expires_at = now + 120_000;
    assert!(
        ingest_nonce_repo::record_once(&replica_a, "token-a", "nonce-a", expires_at).await?
    );
    assert!(
        !ingest_nonce_repo::record_once(&replica_b, "token-a", "nonce-a", expires_at).await?
    );
    assert!(
        ingest_nonce_repo::record_once(&replica_b, "token-a", "nonce-b", expires_at).await?
    );

    assert_eq!(ingest_nonce_repo::cleanup_expired(&replica_a, now).await?, 0);
    assert_eq!(
        ingest_nonce_repo::cleanup_expired(&replica_a, expires_at).await?,
        2
    );
    assert!(
        ingest_nonce_repo::record_once(
            &replica_b,
            "token-a",
            "nonce-a",
            expires_at + 120_000,
        )
        .await?
    );

    Ok(())
}

#[tokio::test]
async fn signed_ingest_replay_is_rejected_across_independent_installed_states(
) -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("signed-replay.sqlite");
    let database_url = format!(
        "sqlite://{}?mode=rwc",
        path.to_string_lossy().replace('\\', "/")
    );
    let replica_a = database::connect(&database_url).await?;
    database::migrate(&replica_a).await?;
    let replica_b = database::connect(&database_url).await?;

    let config = InstallationConfig {
        database_url,
        locale: "en".into(),
        timezone: "UTC".into(),
        secure_cookie: false,
    };
    let pepper = b"test-secret-pepper-32-bytes-long!";
    let state_a = InstalledState::new(
        replica_a,
        config.clone(),
        Arc::new(AuthSecurity::new(pepper)?),
    );
    let state_b = InstalledState::new(
        replica_b,
        config,
        Arc::new(AuthSecurity::new(pepper)?),
    );

    let user_agent = "SondeReplayTest/1.0";
    let client_binding = state_a
        .auth_security
        .ingest
        .client_binding(user_agent, pepper);
    let (token, signing_key, _) = state_a.auth_security.ingest.issue_ingest_token(
        "app-1",
        "env-1",
        "device-12345678",
        &client_binding,
        &["telemetry.events".into()],
        120,
        pepper,
    )?;

    let timestamp = chrono::Utc::now().timestamp_millis();
    let timestamp_header = timestamp.to_string();
    let nonce = "nonce-cross-replica-0001";
    let path = "/api/v1/ingest/events";
    let body = br#"{"items":[]}"#;
    let signature = sign(&signing_key, timestamp, nonce, "POST", path, body)?;

    let first = telemetry::scope_from_context_with_permission(
        &state_a,
        request_context(
            &token,
            &signature,
            &timestamp_header,
            nonce,
            user_agent,
            path,
        ),
        "telemetry.events",
        body,
    )
    .await;
    assert!(first.is_ok());

    let replay = telemetry::scope_from_context_with_permission(
        &state_b,
        request_context(
            &token,
            &signature,
            &timestamp_header,
            nonce,
            user_agent,
            path,
        ),
        "telemetry.events",
        body,
    )
    .await;
    assert!(matches!(replay, Err(AppError::Forbidden)));

    Ok(())
}

fn request_context<'a>(
    token: &'a str,
    signature: &'a str,
    timestamp: &'a str,
    nonce: &'a str,
    user_agent: &'a str,
    path: &'a str,
) -> IngestRequestContext<'a> {
    IngestRequestContext {
        client_ip: "127.0.0.1",
        user_agent,
        credential: Some(token),
        signature: Some(signature),
        timestamp: Some(timestamp),
        nonce: Some(nonce),
        method: "POST",
        path,
    }
}

fn sign(
    signing_key: &str,
    timestamp: i64,
    nonce: &str,
    method: &str,
    path: &str,
    body: &[u8],
) -> Result<String, Box<dyn std::error::Error>> {
    let body_hash = hex::encode(Sha256::digest(body));
    let canonical = format!(
        "sonde-hmac-sha256-v2\n{timestamp}\n{nonce}\n{}\n{path}\n{body_hash}",
        method.to_ascii_uppercase(),
    );
    let mut mac = HmacSha256::new_from_slice(signing_key.as_bytes())
        .map_err(|_| std::io::Error::other("invalid HMAC signing key"))?;
    mac.update(canonical.as_bytes());
    Ok(hex::encode(mac.finalize().into_bytes()))
}
