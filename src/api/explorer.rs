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
    let (installed, user, filter) = authorize(&state, &request, query.into_inner()).await?;
    Ok(HttpResponse::Ok().json(explorer::events(&installed, &user, &filter).await?))
}

async fn metrics(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    query: web::Query<ExplorerQuery>,
) -> Result<HttpResponse, AppError> {
    let (installed, user, filter) = authorize(&state, &request, query.into_inner()).await?;
    Ok(HttpResponse::Ok().json(explorer::metrics(&installed, &user, &filter).await?))
}

async fn logs(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    query: web::Query<ExplorerQuery>,
) -> Result<HttpResponse, AppError> {
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
    Ok((
        installed,
        user,
        ExplorerFilter {
            application_id: query.application_id,
            environment_id: query.environment_id,
            from: query.from,
            to: query.to,
            name: clean(query.name),
            level: clean(query.level),
            text: clean(query.text),
            page: query.page.unwrap_or(1).max(1),
            page_size: query.page_size.unwrap_or(50).clamp(1, 200),
        },
    ))
}

fn clean(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}
