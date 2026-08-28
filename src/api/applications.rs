use std::sync::Arc;

use actix_web::{HttpRequest, HttpResponse, web};
use serde::Deserialize;

use crate::{
    error::AppError,
    services::{applications, authentication, statistics},
    state::AppState,
};

#[derive(Deserialize)]
struct CreateApplication {
    name: String,
    slug: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateApplication {
    name: String,
    slug: String,
    retention_days: Option<i32>,
    is_public: Option<bool>,
    description: Option<Option<String>>,
    github_url: Option<Option<String>>,
    website_url: Option<Option<String>>,
    custom_header: Option<Option<String>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GrantMember {
    user_id: String,
    role: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateKey {
    environment_id: String,
    name: String,
    scopes: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StatsQuery {
    days: Option<u32>,
    environment_id: Option<String>,
}

pub fn configure(config: &mut web::ServiceConfig) {
    config.service(
        web::scope("/api/v1/admin/applications")
            .route("", web::get().to(list))
            .route("", web::post().to(create))
            .route(
                "/{application_id}/environments",
                web::get().to(list_environments),
            )
            .route("/{application_id}", web::patch().to(update))
            .route("/{application_id}", web::delete().to(delete))
            .route("/{application_id}/members", web::get().to(list_members))
            .route("/{application_id}/members", web::post().to(grant_member))
            .route(
                "/{application_id}/members/{user_id}",
                web::delete().to(revoke_member),
            )
            .route("/{application_id}/keys", web::get().to(list_keys))
            .route("/{application_id}/keys", web::post().to(create_key))
            .route(
                "/{application_id}/keys/revoked",
                web::delete().to(clear_revoked_keys),
            )
            .route(
                "/{application_id}/keys/{key_id}/revoke",
                web::post().to(revoke_key),
            )
            .route(
                "/{application_id}/keys/{key_id}",
                web::delete().to(delete_key),
            )
            .route(
                "/{application_id}/keys/{key_id}/regenerate",
                web::post().to(regenerate_key),
            )
            .route("/{application_id}/stats", web::get().to(stats)),
    );
}

async fn list_environments(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate(&installed, &request).await?;
    Ok(HttpResponse::Ok()
        .json(applications::list_environments(&installed, &user, path.as_str()).await?))
}

async fn list(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate(&installed, &request).await?;
    Ok(HttpResponse::Ok().json(applications::list(&installed, &user).await?))
}

async fn create(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    body: web::Json<CreateApplication>,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate_mutation(&installed, &request).await?;
    let (application_id, environment_id) =
        applications::create(&installed, &user, &body.name, &body.slug).await?;
    Ok(HttpResponse::Created()
        .json(serde_json::json!({ "id": application_id, "environmentId": environment_id })))
}

async fn update(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    path: web::Path<String>,
    body: web::Json<UpdateApplication>,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate_mutation(&installed, &request).await?;
    applications::update(
        &installed,
        &user,
        &path,
        applications::UpdateApplicationParams {
            name: &body.name,
            slug: &body.slug,
            retention_days: body.retention_days.unwrap_or(365),
            is_public: body.is_public,
            description: body.description.clone(),
            github_url: body.github_url.clone(),
            website_url: body.website_url.clone(),
            custom_header: body.custom_header.clone(),
        },
    )
    .await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "ok": true })))
}

async fn delete(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate_mutation(&installed, &request).await?;
    applications::delete(&installed, &user, &path).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "ok": true })))
}

async fn list_members(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate(&installed, &request).await?;
    let members = applications::list_members(&installed, &user, &path).await?;
    Ok(HttpResponse::Ok().json(members))
}

async fn grant_member(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    path: web::Path<String>,
    body: web::Json<GrantMember>,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate_mutation(&installed, &request).await?;
    let role = body.role.as_deref().unwrap_or("Manager");
    applications::grant_member(&installed, &user, &path, &body.user_id, role).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "ok": true })))
}

async fn revoke_member(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    path: web::Path<(String, String)>,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate_mutation(&installed, &request).await?;
    let (app_id, user_id) = path.into_inner();
    applications::revoke_member(&installed, &user, &app_id, &user_id).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "ok": true })))
}

async fn list_keys(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate(&installed, &request).await?;
    Ok(HttpResponse::Ok().json(applications::list_keys(&installed, &user, &path).await?))
}

async fn create_key(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    path: web::Path<String>,
    body: web::Json<CreateKey>,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate_mutation(&installed, &request).await?;
    let key = applications::create_key(
        &installed,
        &user,
        &path,
        &body.environment_id,
        &body.name,
        &body.scopes,
    )
    .await?;
    Ok(HttpResponse::Created().json(serde_json::json!({ "key": key, "shownOnce": true })))
}

async fn revoke_key(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    path: web::Path<(String, String)>,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate_mutation(&installed, &request).await?;
    let (app_id, key_id) = path.into_inner();
    applications::revoke_key(&installed, &user, &app_id, &key_id).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "ok": true })))
}

async fn delete_key(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    path: web::Path<(String, String)>,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate_mutation(&installed, &request).await?;
    let (app_id, key_id) = path.into_inner();
    applications::delete_key(&installed, &user, &app_id, &key_id).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "ok": true })))
}

async fn clear_revoked_keys(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate_mutation(&installed, &request).await?;
    let app_id = path.into_inner();
    let deleted_count = applications::clear_revoked_keys(&installed, &user, &app_id).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "deleted": deleted_count })))
}

async fn regenerate_key(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    path: web::Path<(String, String)>,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate_mutation(&installed, &request).await?;
    let (app_id, key_id) = path.into_inner();
    let new_key = applications::regenerate_key(&installed, &user, &app_id, &key_id).await?;
    Ok(HttpResponse::Created().json(serde_json::json!({ "key": new_key, "shownOnce": true })))
}

async fn stats(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    path: web::Path<String>,
    query: web::Query<StatsQuery>,
) -> Result<HttpResponse, AppError> {
    let _permit = state.try_acquire_analytics()?;
    let installed = state.installed().await?;
    let user = authentication::authenticate(&installed, &request).await?;
    let stats = statistics::application_stats(
        &installed,
        &user,
        &path,
        query.environment_id.as_deref(),
        query.days,
    )
    .await?;
    Ok(HttpResponse::Ok().json(stats))
}
