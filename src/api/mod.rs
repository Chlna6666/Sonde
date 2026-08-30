mod access;
mod alerts;
mod applications;
mod authentication;
mod backup;
mod devices;
mod error_response;
mod errors;
mod explorer;
mod ingest;
mod migrations;
mod public;
mod request_auth;
mod setup;
mod stats;
mod stream;

use std::sync::Arc;

use actix_web::{HttpResponse, web};

use crate::state::AppState;

pub(crate) use error_response::{json_config, path_config, query_config};

pub fn configure(config: &mut web::ServiceConfig) {
    config
        .route("/health/live", web::get().to(|| async { "ok" }))
        .route("/health/ready", web::get().to(readiness))
        .configure(setup::configure)
        .configure(authentication::configure)
        .configure(access::configure)
        .configure(applications::configure)
        .configure(devices::configure)
        .configure(backup::configure)
        .configure(public::configure)
        .configure(explorer::configure)
        .configure(errors::configure)
        .configure(ingest::configure)
        .configure(migrations::configure)
        .configure(stats::configure)
        .configure(alerts::configure)
        .configure(stream::configure);
}

async fn readiness(state: web::Data<Arc<AppState>>) -> HttpResponse {
    let Ok(installed) = state.installed().await else {
        return HttpResponse::ServiceUnavailable().body("not initialized");
    };
    match installed.database.ping().await {
        Ok(()) => HttpResponse::Ok().body("ok"),
        Err(_) => HttpResponse::ServiceUnavailable().body("database unavailable"),
    }
}
