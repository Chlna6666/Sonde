use std::sync::Arc;

use actix_web::{HttpRequest, HttpResponse, http::header, web};
use serde::de::DeserializeOwned;

use crate::{
    domain::telemetry::{Batch, ErrorInput, EventInput, LogInput, MetricInput},
    error::AppError,
    services::telemetry::{self, IngestRequestContext, IngestTokenContext, IngestTokenRequest},
    state::AppState,
};

use super::request_auth::bearer_token;

const MAX_INGEST_BODY_BYTES: usize = 1_048_576;
const MAX_TOKEN_BODY_BYTES: usize = 16_384;

pub fn configure(config: &mut web::ServiceConfig) {
    config.service(
        web::scope("/api/v1/ingest")
            .app_data(web::PayloadConfig::new(MAX_INGEST_BODY_BYTES))
            .app_data(web::JsonConfig::default().limit(MAX_TOKEN_BODY_BYTES))
            .route("/token", web::post().to(token))
            .route("/events", web::post().to(events))
            .route("/metrics", web::post().to(metrics))
            .route("/logs", web::post().to(logs))
            .route("/errors", web::post().to(errors)),
    );
}

async fn token(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    body: web::Json<IngestTokenRequest>,
) -> Result<HttpResponse, AppError> {
    let _permit = state.try_acquire_ingest()?;
    let installed = state.installed().await?;
    let client_ip = extract_client_ip(&request);
    let context = IngestTokenContext {
        client_ip: &client_ip,
        user_agent: user_agent(&request),
        raw_key: extract_raw_key(&request),
    };
    let response = telemetry::issue_token(&installed, context, body.into_inner()).await?;
    Ok(HttpResponse::Ok().json(response))
}

async fn events(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    body: web::Bytes,
) -> Result<HttpResponse, AppError> {
    ensure_body_size(&body)?;
    let _permit = state.try_acquire_ingest()?;
    let installed = state.installed().await?;
    let client_ip = extract_client_ip(&request);
    let scope = telemetry::scope_from_context_with_permission(
        &installed,
        ingest_request_context(&request, &client_ip),
        "telemetry.events",
        body.as_ref(),
    )
    .await?;
    let batch: Batch<EventInput> = parse_batch(&body)?;
    let receipt = telemetry::events(&installed, &scope, batch.items).await?;
    publish(&state, &scope.application_id, "events", receipt.accepted);
    Ok(HttpResponse::Accepted().json(receipt))
}

async fn metrics(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    body: web::Bytes,
) -> Result<HttpResponse, AppError> {
    ensure_body_size(&body)?;
    let _permit = state.try_acquire_ingest()?;
    let installed = state.installed().await?;
    let client_ip = extract_client_ip(&request);
    let scope = telemetry::scope_from_context_with_permission(
        &installed,
        ingest_request_context(&request, &client_ip),
        "telemetry.metrics",
        body.as_ref(),
    )
    .await?;
    let batch: Batch<MetricInput> = parse_batch(&body)?;
    let receipt = telemetry::metrics(&installed, &scope, batch.items).await?;
    publish(&state, &scope.application_id, "metrics", receipt.accepted);
    Ok(HttpResponse::Accepted().json(receipt))
}

async fn logs(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    body: web::Bytes,
) -> Result<HttpResponse, AppError> {
    ensure_body_size(&body)?;
    let _permit = state.try_acquire_ingest()?;
    let installed = state.installed().await?;
    let client_ip = extract_client_ip(&request);
    let scope = telemetry::scope_from_context_with_permission(
        &installed,
        ingest_request_context(&request, &client_ip),
        "telemetry.logs",
        body.as_ref(),
    )
    .await?;
    let batch: Batch<LogInput> = parse_batch(&body)?;
    let receipt = telemetry::logs(&installed, &scope, batch.items).await?;
    publish(&state, &scope.application_id, "logs", receipt.accepted);
    Ok(HttpResponse::Accepted().json(receipt))
}

async fn errors(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    body: web::Bytes,
) -> Result<HttpResponse, AppError> {
    ensure_body_size(&body)?;
    let _permit = state.try_acquire_ingest()?;
    let installed = state.installed().await?;
    let client_ip = extract_client_ip(&request);
    let scope = telemetry::scope_from_context_with_permission(
        &installed,
        ingest_request_context(&request, &client_ip),
        "telemetry.errors",
        body.as_ref(),
    )
    .await?;
    let batch: Batch<ErrorInput> = parse_batch(&body)?;
    let receipt = telemetry::errors(&installed, &scope, batch.items).await?;
    publish(&state, &scope.application_id, "errors", receipt.accepted);
    Ok(HttpResponse::Accepted().json(receipt))
}

fn ensure_body_size(body: &[u8]) -> Result<(), AppError> {
    if body.len() > MAX_INGEST_BODY_BYTES {
        return Err(AppError::PayloadTooLarge);
    }
    Ok(())
}

fn parse_batch<T: DeserializeOwned>(body: &[u8]) -> Result<Batch<T>, AppError> {
    serde_json::from_slice(body)
        .map_err(|_| AppError::Validation("invalid telemetry JSON payload".into()))
}

fn publish(state: &AppState, application_id: &str, kind: &str, accepted: usize) {
    let message =
        serde_json::json!({ "applicationId": application_id, "kind": kind, "accepted": accepted })
            .to_string();
    let _ = state.live_updates.send(message);
}

/// Return the transport peer address used by Actix.
///
/// Forwarded/X-Forwarded-For are deliberately not trusted by default: accepting them without a
/// configured trusted-proxy boundary would allow a direct client to bypass IP based throttling by
/// spoofing request headers. Reverse proxies should therefore enforce rate limits themselves until
/// Sonde grows an explicit trusted-proxy configuration.
fn extract_client_ip(request: &HttpRequest) -> String {
    request
        .peer_addr()
        .map(|address| address.ip().to_string())
        .unwrap_or_else(|| "unknown".into())
}

fn user_agent(request: &HttpRequest) -> &str {
    request
        .headers()
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
}

fn extract_raw_key(request: &HttpRequest) -> Option<&str> {
    bearer_token(request)
        .or_else(|| optional_header(request, "x-sonde-token"))
        .or_else(|| optional_header(request, "x-sonde-key"))
        .or_else(|| optional_header(request, "x-api-key"))
}

fn ingest_request_context<'a>(
    request: &'a HttpRequest,
    client_ip: &'a str,
) -> IngestRequestContext<'a> {
    IngestRequestContext {
        client_ip,
        user_agent: user_agent(request),
        credential: extract_raw_key(request),
        signature: optional_header(request, "x-sonde-signature"),
        timestamp: optional_header(request, "x-sonde-timestamp"),
        nonce: optional_header(request, "x-sonde-nonce"),
        method: request.method().as_str(),
        path: request.path(),
    }
}

fn optional_header<'a>(request: &'a HttpRequest, name: &str) -> Option<&'a str> {
    request
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
}
