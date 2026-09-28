use std::sync::Arc;

use actix_web::{HttpRequest, HttpResponse, web};
use serde::{Deserialize, Serialize};

use crate::{
    error::AppError,
    services::setup::{self, SetupInput},
    state::AppState,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SetupStatus {
    installed: bool,
    database_types: &'static [&'static str],
    version: &'static str,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TestRequest {
    database_type: String,
    database_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompleteRequest {
    database_type: String,
    database_url: Option<String>,
    locale: String,
    timezone: String,
    email: String,
    username: String,
    password: String,
    #[serde(default)]
    secure_cookie: bool,
}

pub fn configure(config: &mut web::ServiceConfig) {
    config.service(
        web::scope("/api/v1/setup")
            .route("/status", web::get().to(status))
            .route("/test", web::post().to(test))
            .route("/complete", web::post().to(complete)),
    );
}

async fn status(state: web::Data<Arc<AppState>>) -> HttpResponse {
    let installed = state.is_installed().await;
    if installed {
        HttpResponse::Ok().json(SetupStatus {
            installed: true,
            database_types: &[],
            version: crate::VERSION,
        })
    } else {
        HttpResponse::Ok().json(SetupStatus {
            installed: false,
            database_types: &["sqlite", "postgresql", "mysql"],
            version: crate::VERSION,
        })
    }
}

async fn test(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    body: web::Json<TestRequest>,
) -> Result<HttpResponse, AppError> {
    if state.is_installed().await {
        return Err(AppError::NotFound);
    }
    require_same_origin(&state, &request)?;
    state.verify_setup_code(setup_code(&request)).await?;
    require_setup_budget(&state, &request).await?;
    setup::test_connection(&state, &body.database_type, body.database_url.as_deref()).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "ok": true })))
}

async fn complete(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    body: web::Json<CompleteRequest>,
) -> Result<HttpResponse, AppError> {
    if state.is_installed().await {
        return Err(AppError::NotFound);
    }
    require_same_origin(&state, &request)?;
    state.verify_setup_code(setup_code(&request)).await?;
    require_setup_budget(&state, &request).await?;
    let _guard = state.setup_lock.lock().await;
    if state.is_installed().await {
        return Err(AppError::NotFound);
    }
    setup::complete(
        &state,
        SetupInput {
            database_type: &body.database_type,
            database_url: body.database_url.as_deref(),
            locale: &body.locale,
            timezone: &body.timezone,
            email: &body.email,
            username: &body.username,
            password: &body.password,
            secure_cookie: body.secure_cookie,
        },
    )
    .await?;
    // The wizard is done: the verification code must not be replayable even if the state flip races.
    state.consume_setup_code().await;
    Ok(HttpResponse::Created().json(serde_json::json!({ "installed": true })))
}

/// Reads the one-time setup verification code from the dedicated header.
fn setup_code(request: &HttpRequest) -> Option<&str> {
    request
        .headers()
        .get("x-sonde-setup-code")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

/// Setup runs before any account exists, so the per-source budget is the only throttle
/// available: `test` makes the server open a database connection to a caller-supplied URL.
async fn require_setup_budget(
    state: &web::Data<Arc<AppState>>,
    request: &HttpRequest,
) -> Result<(), AppError> {
    let client_ip = super::request_auth::client_ip(request, &state.runtime.trusted_proxies);
    if state.charge_setup_request(&client_ip).await {
        Ok(())
    } else {
        Err(AppError::TooManyRequests)
    }
}

/// Setup is gated by the one-time setup token, not by this check. The browser origin must still
/// match the deployment origin exactly. Trusted proxy headers may describe the public scheme/host;
/// untrusted peers cannot redefine either value.
fn require_same_origin(
    state: &web::Data<Arc<AppState>>,
    request: &HttpRequest,
) -> Result<(), AppError> {
    let origin = request
        .headers()
        .get("origin")
        .and_then(|value| value.to_str().ok())
        .ok_or(AppError::Forbidden)?;
    let fallback_scheme = if state.runtime.requires_secure_cookies() {
        "https"
    } else {
        "http"
    };
    super::request_auth::origin_matches_request(
        request,
        origin,
        &state.runtime.trusted_proxies,
        fallback_scheme,
        state.runtime.domain.as_deref(),
        &state.runtime.allowed_hosts,
    )
    .then_some(())
    .ok_or(AppError::Forbidden)
}
