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
        })
    } else {
        HttpResponse::Ok().json(SetupStatus {
            installed: false,
            database_types: &["sqlite", "postgresql", "mysql"],
        })
    }
}

async fn test(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    body: web::Json<TestRequest>,
) -> Result<HttpResponse, AppError> {
    require_same_origin(&request)?;
    if state.is_installed().await {
        return Err(AppError::NotFound);
    }
    setup::test_connection(&state, &body.database_type, body.database_url.as_deref()).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "ok": true })))
}

async fn complete(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    body: web::Json<CompleteRequest>,
) -> Result<HttpResponse, AppError> {
    require_same_origin(&request)?;
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
    Ok(HttpResponse::Created().json(serde_json::json!({ "installed": true })))
}

fn require_same_origin(request: &HttpRequest) -> Result<(), AppError> {
    let origin = request
        .headers()
        .get("origin")
        .and_then(|value| value.to_str().ok())
        .ok_or(AppError::Forbidden)?;
    let connection = request.connection_info();
    let expected = format!("{}://{}", connection.scheme(), connection.host());
    (origin.trim_end_matches('/') == expected)
        .then_some(())
        .ok_or(AppError::Forbidden)
}
