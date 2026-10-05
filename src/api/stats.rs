use std::sync::Arc;

use actix_web::{HttpRequest, HttpResponse, web};
use serde::Deserialize;

use crate::{
    error::AppError,
    services::{authentication, statistics},
    state::AppState,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OverviewQuery {
    days: Option<u32>,
    #[serde(alias = "application_id")]
    application_id: Option<String>,
}

pub fn configure(config: &mut web::ServiceConfig) {
    config.route("/api/v1/admin/overview", web::get().to(overview));
}

async fn overview(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    query: web::Query<OverviewQuery>,
) -> Result<HttpResponse, AppError> {
    let _permit = state.try_acquire_analytics()?;
    let installed = state.installed().await?;
    let user = authentication::authenticate(&installed, &request).await?;
    let app_id = query.application_id.as_deref().filter(|s| !s.is_empty());
    Ok(HttpResponse::Ok().json(statistics::overview(&installed, &user, app_id, query.days).await?))
}
