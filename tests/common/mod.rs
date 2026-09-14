//! Shared helpers for Sonde integration tests.
//!
//! Integration crates cannot see private items. Keep helpers here on the public
//! `sonde` API so individual `tests/*.rs` files stay focused on one contract.

#![allow(dead_code, clippy::expect_used, clippy::unwrap_used)]

use hmac::{Hmac, Mac};
use sea_orm::DatabaseConnection;
use sha2::{Digest, Sha256};
use sonde::{
    auth,
    config::{InstallationConfig, PasswordPepper, RuntimeConfig},
    database::{self, applications as application_store, auth as auth_store, auth_state},
    state::AppState,
};

pub const TEST_PASSWORD: &str = "SuperSecret123!";
pub const TEST_USER_AGENT: &str = "SondeConcurrencyTest/1.0";
pub const TEST_PEPPER: [u8; 32] = [42; 32];
pub const TEST_INGEST_KEY: &str = "sonde_1234567890abcdef1234567890abcdef";

type HmacSha256 = Hmac<Sha256>;

pub async fn memory_database() -> DatabaseConnection {
    let database = database::connect("sqlite::memory:")
        .await
        .expect("in-memory SQLite should connect");
    database::migrate(&database)
        .await
        .expect("schema migration should complete");
    database
}

pub struct HttpFixture {
    pub _temp_dir: tempfile::TempDir,
    pub state: std::sync::Arc<AppState>,
    pub owner_token: String,
    pub owner_csrf: String,
    pub app_id: String,
    pub env_id: String,
    pub raw_key: String,
}

pub async fn installed_http() -> HttpFixture {
    let temp_dir = tempfile::tempdir().unwrap();
    let data_dir = temp_dir.path().to_path_buf();
    let config_path = data_dir.join("sonde.json");
    let db_path = data_dir.join("sonde.sqlite");
    let db_url = format!(
        "sqlite://{}?mode=rwc",
        db_path.to_string_lossy().replace('\\', "/")
    );

    let install_config = InstallationConfig {
        database_url: db_url.clone(),
        locale: "en".into(),
        timezone: "UTC".into(),
        secure_cookie: false,
    };
    std::fs::write(
        &config_path,
        serde_json::to_string(&install_config).unwrap(),
    )
    .unwrap();

    let runtime = RuntimeConfig {
        bind: "127.0.0.1:8080".into(),
        data_dir,
        config_path,
        database_url_override: None,
        password_pepper: PasswordPepper::new(TEST_PEPPER),
        trusted_proxies: Vec::new(),
        allow_insecure_cookies: true,
    };

    let database = database::connect(&db_url).await.unwrap();
    database::migrate(&database).await.unwrap();
    let password_hash = auth::hash_password(TEST_PASSWORD, &TEST_PEPPER).unwrap();
    auth_store::create_super_admin(
        &database,
        "admin@example.com",
        "admin",
        &password_hash,
        "en",
    )
    .await
    .unwrap();
    let owner = auth_store::user_by_email(&database, "admin@example.com")
        .await
        .unwrap()
        .unwrap();
    let (app_id, env_id) =
        application_store::create_application(&database, "Test App", "test-app", Some(&owner.id))
            .await
            .unwrap();
    let key_hash = hex::encode(Sha256::digest(TEST_INGEST_KEY.as_bytes()));
    application_store::create_api_key(
        &database,
        &app_id,
        &env_id,
        "Concurrency Key",
        &key_hash,
        "sonde_123456",
        &["ingest".to_string()],
    )
    .await
    .unwrap();
    let (owner_token, owner_csrf) = auth_state::create_session(&database, &owner.id)
        .await
        .unwrap();

    HttpFixture {
        _temp_dir: temp_dir,
        state: std::sync::Arc::new(AppState::load(runtime).await.unwrap()),
        owner_token,
        owner_csrf,
        app_id,
        env_id,
        raw_key: TEST_INGEST_KEY.to_owned(),
    }
}

pub fn session_cookie(token: &str) -> actix_web::cookie::Cookie<'static> {
    actix_web::cookie::Cookie::new(auth::DEVELOPMENT_SESSION_COOKIE, token.to_owned())
}

pub fn hmac_hex(
    signing_key: &str,
    timestamp: i64,
    nonce: &str,
    method: &str,
    path: &str,
    body: &[u8],
) -> String {
    let body_hash = hex::encode(Sha256::digest(body));
    let canonical = format!(
        "sonde-hmac-sha256-v2\n{timestamp}\n{nonce}\n{}\n{path}\n{body_hash}",
        method.to_ascii_uppercase(),
    );
    let mut mac = HmacSha256::new_from_slice(signing_key.as_bytes()).expect("hmac key");
    mac.update(canonical.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}
