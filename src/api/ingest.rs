use std::sync::Arc;

use actix_web::{HttpRequest, HttpResponse, web};
use serde::de::DeserializeOwned;

use crate::{
    domain::telemetry::{Batch, ErrorInput, EventInput, LogInput, MetricInput},
    error::AppError,
    services::telemetry::{self, IngestTokenRequest},
    state::AppState,
};

const MAX_INGEST_BODY_BYTES: usize = 1_048_576;

pub fn configure(config: &mut web::ServiceConfig) {
    config.service(
        web::scope("/api/v1/ingest")
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
    let response =
        telemetry::issue_token_from_request(&installed, &request, body.into_inner()).await?;
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
    let scope = telemetry::scope_from_request_with_permission(
        &installed,
        &request,
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
    let scope = telemetry::scope_from_request_with_permission(
        &installed,
        &request,
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
    let scope = telemetry::scope_from_request_with_permission(
        &installed,
        &request,
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
    let scope = telemetry::scope_from_request_with_permission(
        &installed,
        &request,
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
