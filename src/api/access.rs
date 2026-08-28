use std::sync::Arc;

use actix_web::{HttpRequest, HttpResponse, Responder, web};
use serde::Deserialize;

use crate::{
    error::AppError,
    services::{access, authentication},
    state::AppState,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserPayload {
    pub email: String,
    pub username: String,
    pub password: String,
    pub locale: Option<String>,
    pub role: Option<String>,
    pub application_ids: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUserPayload {
    pub email: String,
    pub username: String,
    pub locale: Option<String>,
    pub active: Option<bool>,
    pub role: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetPasswordPayload {
    pub new_password: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignAppsPayload {
    pub application_ids: Vec<String>,
    pub role: Option<String>,
}

pub fn configure(config: &mut web::ServiceConfig) {
    config
        .route("/api/v1/admin/users", web::get().to(list_users))
        .route("/api/v1/admin/users", web::post().to(create_user))
        .route("/api/v1/admin/users/{id}", web::patch().to(update_user))
        .route(
            "/api/v1/admin/users/{id}/password",
            web::post().to(reset_password),
        )
        .route("/api/v1/admin/users/{id}", web::delete().to(delete_user))
        .route(
            "/api/v1/admin/users/{id}/applications",
            web::get().to(get_user_applications),
        )
        .route(
            "/api/v1/admin/users/{id}/applications",
            web::put().to(set_user_applications),
        )
        .route("/api/v1/admin/roles", web::get().to(list_roles))
        .route("/api/v1/admin/audit", web::get().to(list_audit_logs));
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditQuery {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub action: Option<String>,
    pub resource_type: Option<String>,
}

async fn list_audit_logs(
    state: web::Data<Arc<AppState>>,
    req: HttpRequest,
    query: web::Query<AuditQuery>,
) -> Result<impl Responder, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate(&installed, &req).await?;
    let page = query.page.unwrap_or(1);
    let page_size = query.page_size.unwrap_or(50);
    let logs = access::list_audit_logs(
        &installed,
        &user,
        page,
        page_size,
        query.action.as_deref(),
        query.resource_type.as_deref(),
    )
    .await?;
    Ok(HttpResponse::Ok().json(logs))
}

async fn list_users(
    state: web::Data<Arc<AppState>>,
    req: HttpRequest,
) -> Result<impl Responder, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate(&installed, &req).await?;
    let users = access::list_users(&installed, &user).await?;
    Ok(HttpResponse::Ok().json(users))
}

async fn create_user(
    state: web::Data<Arc<AppState>>,
    req: HttpRequest,
    body: web::Json<CreateUserPayload>,
) -> Result<impl Responder, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate_mutation(&installed, &req).await?;
    let role = body.role.as_deref().unwrap_or("User");
    let locale = body.locale.as_deref().unwrap_or("en");
    let user_id = access::create_user(
        &installed,
        &user,
        access::CreateUserInput {
            email: &body.email,
            username: &body.username,
            password: &body.password,
            locale,
            role,
            pepper: state.runtime.password_pepper.as_bytes(),
        },
    )
    .await?;

    if let Some(ref app_ids) = body.application_ids {
        if !app_ids.is_empty() {
            let _ = access::set_user_assigned_applications(
                &installed, &user, &user_id, app_ids, "Manager",
            )
            .await;
        }
    }

    Ok(HttpResponse::Created().json(serde_json::json!({ "id": user_id })))
}

async fn update_user(
    state: web::Data<Arc<AppState>>,
    req: HttpRequest,
    path: web::Path<String>,
    body: web::Json<UpdateUserPayload>,
) -> Result<impl Responder, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate_mutation(&installed, &req).await?;
    let target_user_id = path.into_inner();
    let locale = body.locale.as_deref().unwrap_or("en");
    let active = body.active.unwrap_or(true);
    access::update_user(
        &installed,
        &user,
        &target_user_id,
        access::UpdateUserInput {
            email: &body.email,
            username: &body.username,
            locale,
            active,
            role: body.role.as_deref(),
        },
    )
    .await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "ok": true })))
}

async fn reset_password(
    state: web::Data<Arc<AppState>>,
    req: HttpRequest,
    path: web::Path<String>,
    body: web::Json<ResetPasswordPayload>,
) -> Result<impl Responder, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate_mutation(&installed, &req).await?;
    let target_user_id = path.into_inner();
    access::reset_password(
        &installed,
        &user,
        &target_user_id,
        &body.new_password,
        state.runtime.password_pepper.as_bytes(),
    )
    .await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "ok": true })))
}

async fn delete_user(
    state: web::Data<Arc<AppState>>,
    req: HttpRequest,
    path: web::Path<String>,
) -> Result<impl Responder, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate_mutation(&installed, &req).await?;
    let target_user_id = path.into_inner();
    access::delete_user(&installed, &user, &target_user_id).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "ok": true })))
}

async fn get_user_applications(
    state: web::Data<Arc<AppState>>,
    req: HttpRequest,
    path: web::Path<String>,
) -> Result<impl Responder, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate(&installed, &req).await?;
    let target_user_id = path.into_inner();
    let app_ids =
        access::get_user_assigned_applications(&installed, &user, &target_user_id).await?;
    Ok(HttpResponse::Ok().json(app_ids))
}

async fn set_user_applications(
    state: web::Data<Arc<AppState>>,
    req: HttpRequest,
    path: web::Path<String>,
    body: web::Json<AssignAppsPayload>,
) -> Result<impl Responder, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate_mutation(&installed, &req).await?;
    let target_user_id = path.into_inner();
    let role = body.role.as_deref().unwrap_or("Manager");
    access::set_user_assigned_applications(
        &installed,
        &user,
        &target_user_id,
        &body.application_ids,
        role,
    )
    .await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "ok": true })))
}

async fn list_roles(
    state: web::Data<Arc<AppState>>,
    req: HttpRequest,
) -> Result<impl Responder, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate(&installed, &req).await?;
    let roles = access::list_roles(&installed, &user).await?;
    Ok(HttpResponse::Ok().json(roles))
}
