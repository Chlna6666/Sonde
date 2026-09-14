#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use actix_web::{
    App, test,
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
async fn application_devices_and_backup_routes_are_not_shadowed() {
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

    let (app_id, _env_id) =
        database::applications::create_application(&database, "Test App", "test-app", None)
            .await
            .unwrap();

    let (session_token, _csrf) = auth_state::create_session(&database, &admin.id)
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

    // 1. Test GET /api/v1/admin/applications/{app_id}/devices?page=1&pageSize=50
    let req = test::TestRequest::get()
        .uri(&format!(
            "/api/v1/admin/applications/{app_id}/devices?page=1&pageSize=50"
        ))
        .cookie(actix_web::cookie::Cookie::new(
            auth::DEVELOPMENT_SESSION_COOKIE,
            session_token.clone(),
        ))
        .to_request();

    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        actix_web::http::StatusCode::OK,
        "GET /api/v1/admin/applications/{{id}}/devices must return 200 OK"
    );

    // 2. Test GET /api/v1/admin/applications/{app_id}/export
    let req = test::TestRequest::get()
        .uri(&format!("/api/v1/admin/applications/{app_id}/export"))
        .cookie(actix_web::cookie::Cookie::new(
            auth::DEVELOPMENT_SESSION_COOKIE,
            session_token.clone(),
        ))
        .to_request();

    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        actix_web::http::StatusCode::OK,
        "GET /api/v1/admin/applications/{{id}}/export must return 200 OK"
    );

    // 3. Test POST /api/v1/admin/applications/import (route exists and is not 404)
    let req = test::TestRequest::post()
        .uri("/api/v1/admin/applications/import")
        .cookie(actix_web::cookie::Cookie::new(
            auth::DEVELOPMENT_SESSION_COOKIE,
            session_token.clone(),
        ))
        .insert_header(("x-csrf-token", _csrf.as_str()))
        .set_json(serde_json::json!({ "invalid": true }))
        .to_request();

    let resp = test::call_service(&app, req).await;
    assert_ne!(
        resp.status(),
        actix_web::http::StatusCode::NOT_FOUND,
        "POST /api/v1/admin/applications/import must not return 404"
    );
}
