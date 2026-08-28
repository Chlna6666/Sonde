use std::sync::Arc;

use actix_web::{HttpRequest, HttpResponse, web};
use serde::Deserialize;

use crate::{
    database::explorer_repo::ExplorerFilter,
    error::AppError,
    services::{authentication, explorer},
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
    let application_id = required_limited(query.application_id, 128, "applicationId")?;
    let environment_id = optional_limited(query.environment_id, 128, "environmentId")?;
    let name = optional_limited(query.name, 128, "name")?;
    let text = optional_limited(query.text, 512, "text")?;
    let level = optional_limited(query.level, 16, "level")?;
    if let Some(level) = level.as_deref()
        && !matches!(level, "trace" | "debug" | "info" | "warn" | "error" | "fatal")
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

fn required_limited(value: String, max_len: usize, field: &str) -> Result<String, AppError> {
    let value = value.trim();
    if value.is_empty() || value.len() > max_len {
        return Err(AppError::Validation(format!(
            "{field} must be 1..{max_len} bytes"
        )));
    }
    Ok(value.to_owned())
}

fn optional_limited(
    value: Option<String>,
    max_len: usize,
    field: &str,
) -> Result<Option<String>, AppError> {
    value
        .map(|value| {
            let value = value.trim();
            if value.is_empty() {
                Ok(None)
            } else if value.len() > max_len {
                Err(AppError::Validation(format!(
                    "{field} must be at most {max_len} bytes"
                )))
            } else {
                Ok(Some(value.to_owned()))
            }
        })
        .transpose()
        .map(Option::flatten)
}
