use std::sync::Arc;

use actix_web::{HttpRequest, HttpResponse, http::header, web};
use futures_util::StreamExt;
use serde::de::DeserializeOwned;

use crate::{
    domain::{
        device_facts::DeviceFactsInput,
        telemetry::{Batch, ErrorInput, EventInput, LogInput, MetricInput},
    },
    error::AppError,
    services::telemetry::{self, IngestRequestContext, IngestTokenContext, IngestTokenRequest},
    state::AppState,
};

use super::request_auth::bearer_token;

const MAX_INGEST_BODY_BYTES: usize = 1_048_576;
const MAX_TOKEN_BODY_BYTES: usize = 16_384;
const MAX_DEVICE_FACTS_BODY_BYTES: usize = 16_384;

pub fn configure(config: &mut web::ServiceConfig) {
    config.service(
        web::scope("/api/v1/ingest")
            .app_data(super::json_config(MAX_TOKEN_BODY_BYTES))
            .route("/token", web::post().to(token))
            .route("/heartbeat", web::post().to(heartbeat))
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
    let client_ip = super::request_auth::client_ip(&request, &state.runtime.trusted_proxies);
    let context = IngestTokenContext {
        client_ip: &client_ip,
        user_agent: user_agent(&request),
        raw_key: extract_raw_key(&request),
    };
    let response = telemetry::issue_token(&installed, context, body.into_inner()).await?;
    Ok(HttpResponse::Ok().json(response))
}

async fn heartbeat(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    body: web::Payload,
) -> Result<HttpResponse, AppError> {
    let _permit = state.try_acquire_ingest()?;
    preflight_device_token(&request)?;
    let body = read_body(body, MAX_DEVICE_FACTS_BODY_BYTES).await?;
    let installed = state.installed().await?;
    let client_ip = super::request_auth::client_ip(&request, &state.runtime.trusted_proxies);
    let scope = telemetry::scope_from_context_with_permission(
        &installed,
        ingest_request_context(&request, &client_ip),
        "telemetry.heartbeat",
        body.as_ref(),
    )
    .await?;
    let facts: DeviceFactsInput = parse_json(&body)?;
    telemetry::heartbeat(&installed, &scope, facts).await?;
    publish(&state, &scope.application_id, "heartbeat", 1);
    Ok(HttpResponse::NoContent().finish())
}

async fn events(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    body: web::Payload,
) -> Result<HttpResponse, AppError> {
    let _permit = state.try_acquire_ingest()?;
    preflight_device_token(&request)?;
    let body = read_body(body, MAX_INGEST_BODY_BYTES).await?;
    let installed = state.installed().await?;
    let client_ip = super::request_auth::client_ip(&request, &state.runtime.trusted_proxies);
    let scope = telemetry::scope_from_context_with_permission(
        &installed,
        ingest_request_context(&request, &client_ip),
        "telemetry.events",
        body.as_ref(),
    )
    .await?;
    let batch: Batch<EventInput> = parse_json(&body)?;
    let receipt = telemetry::events(&installed, &scope, batch.items).await?;
    publish(&state, &scope.application_id, "events", receipt.accepted);
    Ok(HttpResponse::Accepted().json(receipt))
}

async fn metrics(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    body: web::Payload,
) -> Result<HttpResponse, AppError> {
    let _permit = state.try_acquire_ingest()?;
    preflight_device_token(&request)?;
    let body = read_body(body, MAX_INGEST_BODY_BYTES).await?;
    let installed = state.installed().await?;
    let client_ip = super::request_auth::client_ip(&request, &state.runtime.trusted_proxies);
    let scope = telemetry::scope_from_context_with_permission(
        &installed,
        ingest_request_context(&request, &client_ip),
        "telemetry.metrics",
        body.as_ref(),
    )
    .await?;
    let batch: Batch<MetricInput> = parse_json(&body)?;
    let receipt = telemetry::metrics(&installed, &scope, batch.items).await?;
    publish(&state, &scope.application_id, "metrics", receipt.accepted);
    Ok(HttpResponse::Accepted().json(receipt))
}

async fn logs(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    body: web::Payload,
) -> Result<HttpResponse, AppError> {
    let _permit = state.try_acquire_ingest()?;
    preflight_device_token(&request)?;
    let body = read_body(body, MAX_INGEST_BODY_BYTES).await?;
    let installed = state.installed().await?;
    let client_ip = super::request_auth::client_ip(&request, &state.runtime.trusted_proxies);
    let scope = telemetry::scope_from_context_with_permission(
        &installed,
        ingest_request_context(&request, &client_ip),
        "telemetry.logs",
        body.as_ref(),
    )
    .await?;
    let batch: Batch<LogInput> = parse_json(&body)?;
    let receipt = telemetry::logs(&installed, &scope, batch.items).await?;
    publish(&state, &scope.application_id, "logs", receipt.accepted);
    Ok(HttpResponse::Accepted().json(receipt))
}

async fn errors(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    body: web::Payload,
) -> Result<HttpResponse, AppError> {
    let _permit = state.try_acquire_ingest()?;
    preflight_device_token(&request)?;
    let body = read_body(body, MAX_INGEST_BODY_BYTES).await?;
    let installed = state.installed().await?;
    let client_ip = super::request_auth::client_ip(&request, &state.runtime.trusted_proxies);
    let scope = telemetry::scope_from_context_with_permission(
        &installed,
        ingest_request_context(&request, &client_ip),
        "telemetry.errors",
        body.as_ref(),
    )
    .await?;
    let batch: Batch<ErrorInput> = parse_json(&body)?;
    let receipt = telemetry::errors(&installed, &scope, batch.items).await?;
    publish(&state, &scope.application_id, "errors", receipt.accepted);
    Ok(HttpResponse::Accepted().json(receipt))
}

async fn read_body(mut payload: web::Payload, max_bytes: usize) -> Result<web::Bytes, AppError> {
    let mut body = web::BytesMut::with_capacity(4_096.min(max_bytes));
    while let Some(chunk) = payload.next().await {
        let chunk = chunk.map_err(|error| {
            AppError::Validation(format!("request body could not be read: {error}"))
        })?;
        if body.len().saturating_add(chunk.len()) > max_bytes {
            return Err(AppError::PayloadTooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body.freeze())
}

fn parse_json<T: DeserializeOwned>(body: &[u8]) -> Result<T, AppError> {
    crate::json::from_slice(body)
        .map_err(|_| AppError::Validation("invalid telemetry JSON payload".into()))
}

fn publish(state: &AppState, application_id: &str, kind: &str, accepted: usize) {
    let mut message = String::with_capacity(64 + application_id.len() + kind.len());
    message.push_str("{\"applicationId\":");
    crate::json::write_string(&mut message, application_id);
    message.push_str(",\"kind\":");
    crate::json::write_string(&mut message, kind);
    message.push_str(",\"accepted\":");
    crate::json::write_usize(&mut message, accepted);
    message.push('}');
    let _ = state.live_updates.send(crate::state::LiveUpdate {
        application_id: application_id.to_owned(),
        payload: message,
    });
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

fn preflight_device_token(request: &HttpRequest) -> Result<(), AppError> {
    match extract_raw_key(request) {
        Some(credential) if credential.starts_with("sndt_") => Ok(()),
        Some(_) => Err(AppError::IngestTokenRequired),
        None => Err(AppError::Unauthorized),
    }
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
