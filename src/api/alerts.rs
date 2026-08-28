use std::sync::Arc;

use actix_web::{HttpRequest, HttpResponse, web};
use serde::Deserialize;

use crate::{
    domain::alert::AlertExpression,
    error::AppError,
    services::{alerts, authentication},
    state::AppState,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuleQuery {
    application_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateRule {
    application_id: String,
    name: String,
    expression: AlertExpression,
    cooldown_seconds: i32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateRule {
    name: String,
    expression: AlertExpression,
    cooldown_seconds: i32,
    #[serde(default = "default_true")]
    enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateChannel {
    name: String,
    kind: String,
    config: serde_json::Value,
    #[serde(default = "default_true")]
    enabled: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateChannel {
    name: String,
    kind: String,
    config: serde_json::Value,
    #[serde(default = "default_true")]
    enabled: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeliveriesQuery {
    limit: Option<u64>,
}

pub fn configure(config: &mut web::ServiceConfig) {
    config.service(
        web::scope("/api/v1/admin/alerts")
            .route("/rules", web::get().to(list_rules))
            .route("/rules", web::post().to(create_rule))
            .route("/rules/{id}", web::patch().to(update_rule))
            .route("/rules/{id}", web::delete().to(delete_rule))
            .route("/channels", web::get().to(list_channels))
            .route("/channels", web::post().to(create_channel))
            .route("/channels/{id}", web::patch().to(update_channel))
            .route("/channels/{id}", web::delete().to(delete_channel))
            .route("/channels/{id}/test", web::post().to(test_channel))
            .route("/deliveries", web::get().to(list_deliveries)),
    );
}

async fn list_rules(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    query: web::Query<RuleQuery>,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate(&installed, &request).await?;
    Ok(HttpResponse::Ok()
        .json(alerts::list(&installed, &user, query.application_id.as_deref()).await?))
}

async fn create_rule(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    body: web::Json<CreateRule>,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate_mutation(&installed, &request).await?;
    let id = alerts::create(
        &installed,
        &user,
        &body.application_id,
        &body.name,
        &body.expression,
        body.cooldown_seconds,
    )
    .await?;
    Ok(HttpResponse::Created().json(serde_json::json!({ "id": id })))
}

async fn update_rule(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    path: web::Path<String>,
    body: web::Json<UpdateRule>,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate_mutation(&installed, &request).await?;
    alerts::update(
        &installed,
        &user,
        &path.into_inner(),
        &body.name,
        &body.expression,
        body.cooldown_seconds,
        body.enabled,
    )
    .await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "ok": true })))
}

async fn delete_rule(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate_mutation(&installed, &request).await?;
    alerts::delete(&installed, &user, &path.into_inner()).await?;
    Ok(HttpResponse::NoContent().finish())
}

async fn list_channels(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate(&installed, &request).await?;
    Ok(HttpResponse::Ok().json(alerts::list_channels(&installed, &user).await?))
}

async fn create_channel(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    body: web::Json<CreateChannel>,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate_mutation(&installed, &request).await?;
    let id = alerts::create_channel(
        &installed,
        &user,
        &body.name,
        &body.kind,
        &body.config,
        body.enabled,
    )
    .await?;
    Ok(HttpResponse::Created().json(serde_json::json!({ "id": id })))
}

async fn update_channel(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    path: web::Path<String>,
    body: web::Json<UpdateChannel>,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate_mutation(&installed, &request).await?;
    alerts::update_channel(
        &installed,
        &user,
        &path.into_inner(),
        &body.name,
        &body.kind,
        &body.config,
        body.enabled,
    )
    .await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "ok": true })))
}

async fn delete_channel(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate_mutation(&installed, &request).await?;
    alerts::delete_channel(&installed, &user, &path.into_inner()).await?;
    Ok(HttpResponse::NoContent().finish())
}

async fn test_channel(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate_mutation(&installed, &request).await?;
    alerts::test_channel(&installed, &user, &path.into_inner()).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "ok": true })))
}

async fn list_deliveries(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    query: web::Query<DeliveriesQuery>,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate(&installed, &request).await?;
    Ok(HttpResponse::Ok().json(alerts::list_deliveries(&installed, &user, query.limit).await?))
}
