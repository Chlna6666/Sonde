use std::sync::Arc;

use actix_web::{HttpResponse, Responder, web};
use serde::{Deserialize, Serialize};

use crate::{
    database::stats_repo,
    error::AppError,
    services::{applications, statistics},
    state::AppState,
};

#[derive(Deserialize)]
struct PublicStatsQuery {
    days: Option<u32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicApplicationDetails {
    #[serde(flatten)]
    pub application: applications::PublicApplicationInfo,
    pub stats: stats_repo::AppTelemetryStats,
}

pub fn configure(config: &mut web::ServiceConfig) {
    config.route(
        "/api/v1/public/applications/{slug}",
        web::get().to(get_public_application),
    );
}

async fn get_public_application(
    state: web::Data<Arc<AppState>>,
    path: web::Path<String>,
    query: web::Query<PublicStatsQuery>,
) -> Result<impl Responder, AppError> {
    let _permit = state.try_acquire_analytics()?;
    let installed = state.installed().await?;
    let slug = path.into_inner();
    let app = applications::public_by_slug(&installed, &slug)
        .await?
        .ok_or(AppError::NotFound)?;
    let days = statistics::validate_days(query.days)?;
    let stats = stats_repo::application_stats(&installed.database, &app.id, None, days).await?;

    Ok(HttpResponse::Ok().json(PublicApplicationDetails {
        application: app,
        stats,
    }))
}
