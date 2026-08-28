use std::sync::Arc;

use actix_web::{HttpRequest, HttpResponse, web};
use serde::Deserialize;

use crate::{
    error::AppError,
    services::{authentication, statistics},
    state::AppState,
};

#[derive(Deserialize)]
struct OverviewQuery {
    days: Option<u32>,
}

pub fn configure(config: &mut web::ServiceConfig) {
    config.route("/api/v1/admin/overview", web::get().to(overview));
}

async fn overview(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    query: web::Query<OverviewQuery>,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate(&installed, &request).await?;
    Ok(HttpResponse::Ok().json(statistics::overview(&installed, &user, query.days).await?))
}
