mod access;
mod alerts;
mod applications;
mod authentication;
mod backup;
mod explorer;
mod ingest;
mod migrations;
mod public;
mod setup;
mod stats;
mod stream;

use actix_web::web;

pub fn configure(config: &mut web::ServiceConfig) {
    config
        .route("/health/live", web::get().to(|| async { "ok" }))
        .configure(setup::configure)
        .configure(authentication::configure)
        .configure(access::configure)
        .configure(applications::configure)
        .configure(backup::configure)
        .configure(public::configure)
        .configure(explorer::configure)
        .configure(ingest::configure)
        .configure(migrations::configure)
        .configure(stats::configure)
        .configure(alerts::configure)
        .configure(stream::configure);
}
