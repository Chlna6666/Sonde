#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use actix_web::{App, test, web::Data};
use sonde::{
    api, auth,
    config::{InstallationConfig, PasswordPepper, RuntimeConfig},
    database::{self, applications as application_store, auth as auth_store, auth_state},
    state::AppState,
};

const PASSWORD: &str = "SuperSecret123!";

struct Fixture {
    _temp_dir: tempfile::TempDir,
    runtime: RuntimeConfig,
    database: sea_orm::DatabaseConnection,
    owner_token: String,
    owner_csrf: String,
    app_id: String,
}

async fn fixture() -> Fixture {
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
    };

    let database = database::connect(&db_url).await.unwrap();
    database::migrate(&database).await.unwrap();
    let password_hash = auth::hash_password(PASSWORD, &pepper).unwrap();
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
    let (app_id, _env_id) =
        application_store::create_application(&database, "Test App", "test-app", Some(&owner.id))
            .await
            .unwrap();
    let (owner_token, owner_csrf) = auth_state::create_session(&database, &owner.id)
        .await
        .unwrap();
    Fixture {
        _temp_dir: temp_dir,
        runtime,
        database,
        owner_token,
        owner_csrf,
        app_id,
    }
}

fn session_cookie(token: &str) -> actix_web::cookie::Cookie<'static> {
    actix_web::cookie::Cookie::new(auth::DEVELOPMENT_SESSION_COOKIE, token.to_owned())
}

#[tokio::test]
async fn application_member_cannot_be_granted_privileged_roles() {
    let fixture = fixture().await;
    let member_hash = auth::hash_password(PASSWORD, &[42_u8; 32]).unwrap();
    let member_id = auth_store::create_user(
        &fixture.database,
        "member@example.com",
        "member",
        &member_hash,
        "en",
        "User",
    )
    .await
    .unwrap();
    let state = Arc::new(AppState::load(fixture.runtime.clone()).await.unwrap());
    let app = test::init_service(
        App::new()
            .app_data(Data::new(state))
            .configure(api::configure),
    )
    .await;

    for role in ["Super Admin", "Admin", "User"] {
        let req = test::TestRequest::post()
            .uri(&format!(
                "/api/v1/admin/applications/{}/members",
                fixture.app_id
            ))
            .cookie(session_cookie(&fixture.owner_token))
            .insert_header(("x-csrf-token", fixture.owner_csrf.as_str()))
            .set_json(serde_json::json!({ "userId": member_id, "role": role }))
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(
            resp.status(),
            actix_web::http::StatusCode::BAD_REQUEST,
            "granting {role} as an application member must be rejected"
        );
    }
}

#[tokio::test]
async fn app_scoped_role_name_does_not_unlock_system_backup() {
    let fixture = fixture().await;
    let member_hash = auth::hash_password(PASSWORD, &[42_u8; 32]).unwrap();
    let member_id = auth_store::create_user(
        &fixture.database,
        "scoped@example.com",
        "scoped",
        &member_hash,
        "en",
        "User",
    )
    .await
    .unwrap();
    application_store::grant_application_access(
        &fixture.database,
        &fixture.app_id,
        &member_id,
        "Manager",
    )
    .await
    .unwrap();
    let roles = auth_store::role_names_for_user(&fixture.database, &member_id)
        .await
        .unwrap();
    assert_eq!(roles, vec!["User".to_owned()]);

    let (session_token, csrf) = auth_state::create_session(&fixture.database, &member_id)
        .await
        .unwrap();
    let state = Arc::new(AppState::load(fixture.runtime.clone()).await.unwrap());
    let app = test::init_service(
        App::new()
            .app_data(Data::new(state))
            .configure(api::configure),
    )
    .await;
    let req = test::TestRequest::get()
        .uri("/api/v1/admin/system/backup")
        .cookie(session_cookie(&session_token))
        .insert_header(("x-csrf-token", csrf.as_str()))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), actix_web::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn only_unscoped_owner_can_create_privileged_users() {
    let fixture = fixture().await;
    let admin_hash = auth::hash_password(PASSWORD, &[42_u8; 32]).unwrap();
    let admin_id = auth_store::create_user(
        &fixture.database,
        "operator@example.com",
        "operator",
        &admin_hash,
        "en",
        "Admin",
    )
    .await
    .unwrap();
    let (admin_token, admin_csrf) = auth_state::create_session(&fixture.database, &admin_id)
        .await
        .unwrap();
    let state = Arc::new(AppState::load(fixture.runtime.clone()).await.unwrap());
    let app = test::init_service(
        App::new()
            .app_data(Data::new(state))
            .configure(api::configure),
    )
    .await;

    let forbidden = test::TestRequest::post()
        .uri("/api/v1/admin/users")
        .cookie(session_cookie(&admin_token))
        .insert_header(("x-csrf-token", admin_csrf.as_str()))
        .set_json(serde_json::json!({
            "email": "elevated@example.com",
            "username": "elevated",
            "password": PASSWORD,
            "role": "Super Admin"
        }))
        .to_request();
    let resp = test::call_service(&app, forbidden).await;
    assert_eq!(resp.status(), actix_web::http::StatusCode::FORBIDDEN);

    let allowed = test::TestRequest::post()
        .uri("/api/v1/admin/users")
        .cookie(session_cookie(&fixture.owner_token))
        .insert_header(("x-csrf-token", fixture.owner_csrf.as_str()))
        .set_json(serde_json::json!({
            "email": "second-owner@example.com",
            "username": "second-owner",
            "password": PASSWORD,
            "role": "Super Admin"
        }))
        .to_request();
    let resp = test::call_service(&app, allowed).await;
    assert_eq!(resp.status(), actix_web::http::StatusCode::CREATED);
}

#[tokio::test]
async fn public_application_urls_reject_non_https() {
    let fixture = fixture().await;
    let state = Arc::new(AppState::load(fixture.runtime.clone()).await.unwrap());
    let app = test::init_service(
        App::new()
            .app_data(Data::new(state))
            .configure(api::configure),
    )
    .await;
    let req = test::TestRequest::patch()
        .uri(&format!("/api/v1/admin/applications/{}", fixture.app_id))
        .cookie(session_cookie(&fixture.owner_token))
        .insert_header(("x-csrf-token", fixture.owner_csrf.as_str()))
        .set_json(serde_json::json!({
            "name": "Test App",
            "slug": "test-app",
            "githubUrl": "javascript:alert(1)",
            "websiteUrl": "http://example.com"
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), actix_web::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn password_reset_revokes_existing_sessions() {
    let fixture = fixture().await;
    let target_hash = auth::hash_password(PASSWORD, &[42_u8; 32]).unwrap();
    let target_id = auth_store::create_user(
        &fixture.database,
        "resetme@example.com",
        "resetme",
        &target_hash,
        "en",
        "User",
    )
    .await
    .unwrap();
    let (target_token, _target_csrf) = auth_state::create_session(&fixture.database, &target_id)
        .await
        .unwrap();
    let state = Arc::new(AppState::load(fixture.runtime.clone()).await.unwrap());
    let app = test::init_service(
        App::new()
            .app_data(Data::new(state))
            .configure(api::configure),
    )
    .await;

    let reset = test::TestRequest::post()
        .uri(&format!("/api/v1/admin/users/{target_id}/password"))
        .cookie(session_cookie(&fixture.owner_token))
        .insert_header(("x-csrf-token", fixture.owner_csrf.as_str()))
        .set_json(serde_json::json!({ "newPassword": "AnotherSecret123!" }))
        .to_request();
    let resp = test::call_service(&app, reset).await;
    assert_eq!(resp.status(), actix_web::http::StatusCode::OK);

    let me = test::TestRequest::get()
        .uri("/api/v1/auth/me")
        .cookie(session_cookie(&target_token))
        .to_request();
    let resp = test::call_service(&app, me).await;
    assert_eq!(resp.status(), actix_web::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn login_challenge_does_not_echo_a_prompt() {
    let fixture = fixture().await;
    let state = Arc::new(AppState::load(fixture.runtime.clone()).await.unwrap());
    let app = test::init_service(
        App::new()
            .app_data(Data::new(state))
            .configure(api::configure),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/api/v1/auth/login")
        .set_json(serde_json::json!({
            "username": "admin",
            "password": "wrong-password-15"
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), actix_web::http::StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["code"], "challenge_required");
    assert!(body.get("challengePrompt").is_none() || body["challengePrompt"].is_null());
    assert!(
        body["challengeId"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );
}
