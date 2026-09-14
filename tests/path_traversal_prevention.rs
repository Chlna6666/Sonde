#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use actix_web::{
    App,
    http::StatusCode,
    test,
    web::{self, Data},
};
use sonde::{
    api, auth,
    config::{InstallationConfig, PasswordPepper, RuntimeConfig},
    database::{self, auth as auth_store, auth_state},
    state::AppState,
    web_assets,
};

#[tokio::test]
async fn path_traversal_attempts_are_blocked_at_all_layers() {
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

    let pepper = [42_u8; 32];
    let runtime = RuntimeConfig {
        bind: "127.0.0.1:8080".into(),
        data_dir,
        config_path,
        database_url_override: None,
        password_pepper: PasswordPepper::new(pepper),
        trusted_proxies: Vec::new(),
        allow_insecure_cookies: true,
    };

    let database = database::connect(&db_url).await.unwrap();
    database::migrate(&database).await.unwrap();

    let password_hash = auth::hash_password("SuperSecret123!", &pepper).unwrap();
    auth_store::create_super_admin(
        &database,
        "admin@example.com",
        "admin",
        &password_hash,
        "en",
    )
    .await
    .unwrap();

    let admin = auth_store::user_by_email(&database, "admin@example.com")
        .await
        .unwrap()
        .unwrap();

    let (session_token, _csrf_token) = auth_state::create_session(&database, &admin.id)
        .await
        .unwrap();

    let state = Arc::new(AppState::load(runtime).await.unwrap());

    let app = test::init_service(
        App::new()
            .app_data(Data::new(state.clone()))
            .configure(api::configure)
            .default_service(web::to(web_assets::serve)),
    )
    .await;

    // 1. Static asset server path traversal blocking
    let traversal_urls = [
        "/../etc/passwd",
        "/..%2f..%2fetc%2fpasswd",
        "/%2e%2e/etc/passwd",
        "/%252e%252e/etc/passwd",
        "/assets/..%2fsecret",
        "/assets\\secret",
        "/assets/%00.js",
    ];

    for url in traversal_urls {
        let req = test::TestRequest::get().uri(url).to_request();
        let res = test::call_service(&app, req).await;
        assert_eq!(
            res.status(),
            StatusCode::BAD_REQUEST,
            "Expected 400 Bad Request for traversal attempt: {url}"
        );
    }

    // 2. API path parameter path traversal blocking
    let malicious_app_ids = [
        "../../etc/passwd",
        "..%2f..%2fpasswd",
        "app/sub",
        "app\\sub",
        "-flag",
        ".hidden",
        "app:colon",
    ];

    for bad_id in malicious_app_ids {
        let encoded_id = bad_id.replace('/', "%2f");
        let uri = format!("/api/v1/admin/applications/{encoded_id}/devices?page=1&pageSize=50");
        let req = test::TestRequest::get()
            .uri(&uri)
            .cookie(actix_web::cookie::Cookie::new(
                auth::DEVELOPMENT_SESSION_COOKIE,
                session_token.clone(),
            ))
            .to_request();

        let res = test::call_service(&app, req).await;
        // Should be rejected by validation with BAD_REQUEST or NOT_FOUND
        assert!(
            res.status() == StatusCode::BAD_REQUEST || res.status() == StatusCode::NOT_FOUND,
            "Expected 400 or 404 for malicious application id: {bad_id}, got: {}",
            res.status()
        );
    }

    // 3. Explorer log query parameter validation
    let req = test::TestRequest::get()
        .uri("/api/v1/admin/explorer/logs?applicationId=..%2f..%2fpasswd")
        .cookie(actix_web::cookie::Cookie::new(
            auth::DEVELOPMENT_SESSION_COOKIE,
            session_token.clone(),
        ))
        .to_request();
    let res = test::call_service(&app, req).await;
    assert_eq!(
        res.status(),
        StatusCode::BAD_REQUEST,
        "Expected 400 Bad Request for invalid application id in query"
    );
}
