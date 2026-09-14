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

pub fn configure(config: &mut web::ServiceConfig) {
    config.service(
        web::scope("/api/v1/admin/explorer")
            .route("/events", web::get().to(events))
            .route("/metrics", web::get().to(metrics))
            .route("/logs", web::get().to(logs)),
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
            page: query.page.unwrap_or(1).max(1),
            page_size: query.page_size.unwrap_or(50).clamp(1, 200),
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
