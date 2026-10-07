use std::sync::Arc;

use actix_web::{HttpRequest, HttpResponse, web};
use serde::Deserialize;

use crate::{
    error::AppError,
    services::{
        authentication,
        explorer::{self, ExplorerFilter},
    },
    state::AppState,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExplorerQuery {
    application_id: String,
    environment_id: Option<String>,
    from: Option<i64>,
    to: Option<i64>,
    name: Option<String>,
    level: Option<String>,
    text: Option<String>,
    page: Option<u64>,
    page_size: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeleteRecordsRequest {
    application_id: String,
    environment_id: Option<String>,
    ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResetRecordsRequest {
    application_id: String,
    environment_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CleanInvalidRequest {
    #[serde(alias = "application_id")]
    application_id: Option<String>,
}

pub fn configure(config: &mut web::ServiceConfig) {
    config.service(
        web::scope("/api/v1/admin/explorer")
            .route("/events", web::get().to(events))
            .route("/metrics", web::get().to(metrics))
            .route("/logs", web::get().to(logs))
            .route("/{kind}/delete", web::post().to(delete_records))
            .route("/{kind}", web::delete().to(delete_records))
            .route("/{kind}/reset", web::post().to(reset_records))
            .route("/{kind}/reset", web::delete().to(reset_records))
            .route(
                "/clean-invalid/preview",
                web::post().to(preview_clean_invalid_data),
            )
            .route(
                "/clean-invalid/preview",
                web::get().to(preview_clean_invalid_data_query),
            )
            .route("/clean-invalid", web::post().to(clean_invalid_data)),
    );
}

async fn events(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    query: web::Query<ExplorerQuery>,
) -> Result<HttpResponse, AppError> {
    let _permit = state.try_acquire_analytics()?;
    let (installed, user, filter) = authorize(&state, &request, query.into_inner()).await?;
    Ok(HttpResponse::Ok().json(explorer::events(&installed, &user, &filter).await?))
}

async fn metrics(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    query: web::Query<ExplorerQuery>,
) -> Result<HttpResponse, AppError> {
    let _permit = state.try_acquire_analytics()?;
    let (installed, user, filter) = authorize(&state, &request, query.into_inner()).await?;
    Ok(HttpResponse::Ok().json(explorer::metrics(&installed, &user, &filter).await?))
}

async fn logs(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    query: web::Query<ExplorerQuery>,
) -> Result<HttpResponse, AppError> {
    let _permit = state.try_acquire_analytics()?;
    let (installed, user, filter) = authorize(&state, &request, query.into_inner()).await?;
    Ok(HttpResponse::Ok().json(explorer::logs(&installed, &user, &filter).await?))
}

async fn delete_records(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    path: web::Path<String>,
    body: web::Json<DeleteRecordsRequest>,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate_mutation(&installed, &request).await?;
    let kind = path.into_inner();
    let body = body.into_inner();
    let deleted = explorer::delete_records(
        &installed,
        &user,
        &kind,
        &body.application_id,
        body.environment_id.as_deref(),
        &body.ids,
    )
    .await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "deleted": deleted })))
}

async fn reset_records(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    path: web::Path<String>,
    body: web::Json<ResetRecordsRequest>,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate_mutation(&installed, &request).await?;
    let kind = path.into_inner();
    let body = body.into_inner();
    let deleted = explorer::reset_records(
        &installed,
        &user,
        &kind,
        &body.application_id,
        body.environment_id.as_deref(),
    )
    .await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "deleted": deleted })))
}

async fn preview_clean_invalid_data(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    body: web::Json<CleanInvalidRequest>,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate(&installed, &request).await?;
    let app_id = body.application_id.as_deref().filter(|s| !s.is_empty());
    let preview = explorer::preview_clean_invalid_data(&installed, &user, app_id).await?;
    Ok(HttpResponse::Ok().json(preview))
}

async fn preview_clean_invalid_data_query(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    query: web::Query<CleanInvalidRequest>,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate(&installed, &request).await?;
    let app_id = query.application_id.as_deref().filter(|s| !s.is_empty());
    let preview = explorer::preview_clean_invalid_data(&installed, &user, app_id).await?;
    Ok(HttpResponse::Ok().json(preview))
}

async fn clean_invalid_data(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    body: web::Json<CleanInvalidRequest>,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate_mutation(&installed, &request).await?;
    let app_id = body.application_id.as_deref().filter(|s| !s.is_empty());
    let result = explorer::clean_invalid_data(&installed, &user, app_id).await?;
    Ok(HttpResponse::Ok().json(result))
}

async fn authorize(
    state: &web::Data<Arc<AppState>>,
    request: &HttpRequest,
    query: ExplorerQuery,
) -> Result<
    (
        Arc<crate::state::InstalledState>,
        authentication::AuthenticatedUser,
        ExplorerFilter,
    ),
    AppError,
> {
    let installed = state.installed().await?;
    let user = authentication::authenticate(&installed, request).await?;
    if matches!((query.from, query.to), (Some(from), Some(to)) if from > to) {
        return Err(AppError::Validation(
            "the start time must be before the end time".into(),
        ));
    }
    let application_id =
        crate::security::validate_safe_identifier("applicationId", &query.application_id)?;
    let environment_id = crate::security::validate_optional_safe_identifier(
        "environmentId",
        query.environment_id.as_deref(),
    )?;
    let name = optional_text(query.name, 128, "name")?;
    let text = optional_text(query.text, 512, "text")?;
    let level = optional_text(query.level, 16, "level")?;
    if let Some(level) = level.as_deref()
        && !matches!(
            level,
            "trace" | "debug" | "info" | "warn" | "error" | "fatal"
        )
    {
        return Err(AppError::Validation("invalid log level".into()));
    }

    Ok((
        installed,
        user,
        ExplorerFilter {
            application_id,
            environment_id,
            from: query.from,
            to: query.to,
            name,
            level,
            text,
            page: crate::security::bounded_page(query.page),
            page_size: crate::security::bounded_page_size(query.page_size),
        },
    ))
}

fn optional_text(
    value: Option<String>,
    max_len: usize,
    field: &str,
) -> Result<Option<String>, AppError> {
    match value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty()) {
        None => Ok(None),
        Some(v) => {
            if v.len() > max_len || v.chars().any(|c| c.is_control() || c == '\0') {
                return Err(AppError::Validation(format!(
                    "{field} is invalid or too long"
                )));
            }
            Ok(Some(v))
        }
    }
}
