use std::sync::Arc;

use actix_web::{HttpRequest, HttpResponse, web};

use crate::{
    domain::telemetry::{Batch, ErrorInput, EventInput, LogInput, MetricInput},
    error::AppError,
    services::telemetry::{self, IngestTokenRequest},
    state::AppState,
};

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
    let installed = state.installed().await?;
    let response = telemetry::issue_token_from_request(&installed, &request, body.into_inner()).await?;
    Ok(HttpResponse::Ok().json(response))
}

async fn events(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    body: web::Json<Batch<EventInput>>,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let scope = telemetry::scope_from_request_with_permission(&installed, &request, "telemetry.events").await?;
    let receipt = telemetry::events(&installed, &scope, body.into_inner().items).await?;
    publish(&state, &scope.application_id, "events", receipt.accepted);
    Ok(HttpResponse::Accepted().json(receipt))
}

async fn metrics(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    body: web::Json<Batch<MetricInput>>,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let scope = telemetry::scope_from_request_with_permission(&installed, &request, "telemetry.metrics").await?;
    let receipt = telemetry::metrics(&installed, &scope, body.into_inner().items).await?;
    publish(&state, &scope.application_id, "metrics", receipt.accepted);
    Ok(HttpResponse::Accepted().json(receipt))
}

async fn logs(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    body: web::Json<Batch<LogInput>>,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let scope = telemetry::scope_from_request_with_permission(&installed, &request, "telemetry.logs").await?;
    let receipt = telemetry::logs(&installed, &scope, body.into_inner().items).await?;
    publish(&state, &scope.application_id, "logs", receipt.accepted);
    Ok(HttpResponse::Accepted().json(receipt))
}

async fn errors(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    body: web::Json<Batch<ErrorInput>>,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let scope = telemetry::scope_from_request_with_permission(&installed, &request, "telemetry.errors").await?;
    let receipt = telemetry::errors(&installed, &scope, body.into_inner().items).await?;
    publish(&state, &scope.application_id, "errors", receipt.accepted);
    Ok(HttpResponse::Accepted().json(receipt))
}

fn publish(state: &AppState, application_id: &str, kind: &str, accepted: usize) {
    let message =
        serde_json::json!({ "applicationId": application_id, "kind": kind, "accepted": accepted })
            .to_string();
    let _ = state.live_updates.send(message);
}
