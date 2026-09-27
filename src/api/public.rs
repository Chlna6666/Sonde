use std::sync::Arc;

use actix_web::{HttpResponse, Responder, web};
use serde::{Deserialize, Serialize};

use crate::{
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
    pub stats: statistics::AppTelemetryStats,
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
    let slug = path.into_inner();
    if slug.len() < 2
        || slug.len() > 64
        || !slug
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(AppError::NotFound);
    }
    let _permit = state.try_acquire_analytics()?;
    let installed = state.installed().await?;
    let app = applications::public_by_slug(&installed, &slug)
        .await?
        .ok_or(AppError::NotFound)?;
    let stats = statistics::public_application_stats(&installed, &app.id, query.days).await?;

    Ok(HttpResponse::Ok().json(PublicApplicationDetails {
        application: app,
        stats,
    }))
}
