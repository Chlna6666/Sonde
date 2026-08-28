use std::sync::Arc;

use actix_web::{HttpRequest, HttpResponse, web};
use serde::Deserialize;

use crate::{
    database::error_query_repo::ErrorGroupFilter,
    error::AppError,
    services::{authentication, errors},
    state::AppState,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GroupsQuery {
    application_id: String,
    environment_id: Option<String>,
    severity: Option<String>,
    from: Option<i64>,
    to: Option<i64>,
    page: Option<u64>,
    page_size: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OccurrencesQuery {
    from: Option<i64>,
    to: Option<i64>,
    page: Option<u64>,
    page_size: Option<u64>,
}

pub fn configure(config: &mut web::ServiceConfig) {
    config.service(
        web::scope("/api/v1/admin/errors")
            .route("/groups", web::get().to(groups))
            .route("/groups/{group_id}", web::get().to(group))
            .route(
                "/groups/{group_id}/occurrences",
                web::get().to(occurrences),
            ),
    );
}

async fn groups(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    query: web::Query<GroupsQuery>,
) -> Result<HttpResponse, AppError> {
    let _permit = state.try_acquire_analytics()?;
    let installed = state.installed().await?;
    let user = authentication::authenticate(&installed, &request).await?;
    let query = query.into_inner();
    validate_identifier("applicationId", &query.application_id)?;
    if let Some(environment_id) = query.environment_id.as_deref() {
        validate_identifier("environmentId", environment_id)?;
    }
    validate_window(query.from, query.to)?;
    let severity = normalize_severity(query.severity)?;
    let filter = ErrorGroupFilter {
        application_id: query.application_id,
        environment_id: clean(query.environment_id),
        severity,
        from: query.from,
        to: query.to,
        page: query.page.unwrap_or(1).max(1),
        page_size: query.page_size.unwrap_or(50).clamp(1, 200),
    };
    Ok(HttpResponse::Ok().json(errors::groups(&installed, &user, &filter).await?))
}

async fn group(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let _permit = state.try_acquire_analytics()?;
    let group_id = path.into_inner();
    validate_identifier("groupId", &group_id)?;
    let installed = state.installed().await?;
    let user = authentication::authenticate(&installed, &request).await?;
    Ok(HttpResponse::Ok().json(errors::group(&installed, &user, &group_id).await?))
}

async fn occurrences(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    path: web::Path<String>,
    query: web::Query<OccurrencesQuery>,
) -> Result<HttpResponse, AppError> {
    let _permit = state.try_acquire_analytics()?;
    let group_id = path.into_inner();
    validate_identifier("groupId", &group_id)?;
    let query = query.into_inner();
    validate_window(query.from, query.to)?;
    let installed = state.installed().await?;
    let user = authentication::authenticate(&installed, &request).await?;
    Ok(HttpResponse::Ok().json(
        errors::occurrences(
            &installed,
            &user,
            &group_id,
            query.page.unwrap_or(1).max(1),
            query.page_size.unwrap_or(50).clamp(1, 200),
            query.from,
            query.to,
        )
        .await?,
    ))
}

fn normalize_severity(value: Option<String>) -> Result<Option<String>, AppError> {
    let Some(value) = clean(value) else {
        return Ok(None);
    };
    let normalized = value.to_ascii_lowercase();
    if !matches!(normalized.as_str(), "warning" | "error" | "fatal") {
        return Err(AppError::Validation(
            "severity must be warning, error, or fatal".into(),
        ));
    }
    Ok(Some(normalized))
}

fn validate_window(from: Option<i64>, to: Option<i64>) -> Result<(), AppError> {
    if matches!((from, to), (Some(from), Some(to)) if from > to) {
        return Err(AppError::Validation(
            "the start time must be before the end time".into(),
        ));
    }
    Ok(())
}

fn validate_identifier(name: &str, value: &str) -> Result<(), AppError> {
    if value.is_empty() || value.len() > 128 {
        return Err(AppError::Validation(format!(
            "{name} must be 1..128 bytes"
        )));
    }
    Ok(())
}

fn clean(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim();
        if value.is_empty() {
            None
        } else {
            Some(value.to_owned())
        }
    })
}
