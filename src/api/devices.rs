use std::sync::Arc;

use actix_web::{HttpRequest, HttpResponse, web};
use serde::Deserialize;

use crate::{
    error::AppError,
    services::{authentication, devices},
    state::AppState,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeviceQueryParams {
    environment_id: Option<String>,
    status: Option<String>,
    risk: Option<String>,
    min_risk: Option<i32>,
    search: Option<String>,
    page: Option<u64>,
    page_size: Option<u64>,
}

pub(crate) async fn list(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    path: web::Path<String>,
    query: web::Query<DeviceQueryParams>,
) -> Result<HttpResponse, AppError> {
    let _permit = state.try_acquire_analytics()?;
    let installed = state.installed().await?;
    let user = authentication::authenticate(&installed, &request).await?;
    let application_id =
        crate::security::validate_safe_identifier("applicationId", &path.into_inner())?;
    let query = query.into_inner();
    let environment_id = crate::security::validate_optional_safe_identifier(
        "environmentId",
        query.environment_id.as_deref(),
    )?;
    let search = optional_text(query.search, 128, "search")?;
    let status = parse_status(query.status.as_deref())?;
    let risk = parse_risk(query.risk.as_deref())?;
    let min_risk = query
        .min_risk
        .map(|value| {
            if (0..=100).contains(&value) {
                Ok(value)
            } else {
                Err(AppError::Validation(
                    "minRisk must be between 0 and 100".into(),
                ))
            }
        })
        .transpose()?;

    let result = devices::list(
        &installed,
        &user,
        devices::Query {
            application_id,
            environment_id,
            status,
            risk,
            min_risk,
            search,
            page: crate::security::bounded_page(query.page),
            page_size: crate::security::bounded_page_size(query.page_size),
        },
    )
    .await?;
    Ok(HttpResponse::Ok().json(result))
}

fn parse_status(value: Option<&str>) -> Result<Option<devices::StatusFilter>, AppError> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        None => Ok(None),
        Some("active") => Ok(Some(devices::StatusFilter::Active)),
        Some("recent") => Ok(Some(devices::StatusFilter::Recent)),
        Some("offline") => Ok(Some(devices::StatusFilter::Offline)),
        Some(_) => Err(AppError::Validation(
            "status must be active, recent, or offline".into(),
        )),
    }
}

fn parse_risk(value: Option<&str>) -> Result<Option<devices::RiskFilter>, AppError> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        None => Ok(None),
        Some("low") => Ok(Some(devices::RiskFilter::Low)),
        Some("medium") => Ok(Some(devices::RiskFilter::Medium)),
        Some("high") => Ok(Some(devices::RiskFilter::High)),
        Some("critical") => Ok(Some(devices::RiskFilter::Critical)),
        Some("risky") => Ok(Some(devices::RiskFilter::Risky)),
        Some(_) => Err(AppError::Validation(
            "risk must be low, medium, high, critical, or risky".into(),
        )),
    }
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
