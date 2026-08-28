pub mod api;
pub mod auth;
pub mod config;
pub mod database;
pub mod domain;
pub mod error;
pub mod security;
pub mod services;
pub mod state;
pub mod totp;
pub mod web_assets;

use std::{io, sync::Arc};

use actix_web::{App, HttpServer, middleware, web};
use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::{config::RuntimeConfig, state::AppState};

pub async fn run() -> io::Result<()> {
    init_tracing();
    let runtime = RuntimeConfig::from_environment()?;
    let state = Arc::new(
        AppState::load(runtime.clone())
            .await
            .map_err(io::Error::other)?,
    );

    if let Ok(installed) = state.installed().await {
        services::retention::spawn_retention_worker(installed.database.clone());
        services::alerts::spawn_alert_evaluator_worker(installed.database.clone());
    } else {
        info!("Sonde is waiting for one-time web setup");
    }
    info!(address = %runtime.bind, "starting Sonde");

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(state.clone()))
            .app_data(web::JsonConfig::default().limit(2_097_152))
            .app_data(web::PayloadConfig::default().limit(4_194_304))
            .wrap(
                middleware::DefaultHeaders::new()
                    .add(("content-security-policy", "default-src 'self'; script-src 'self'; style-src 'self'; font-src 'self' data:; img-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'self'; frame-ancestors 'none'; form-action 'self'"))
                    .add(("x-content-type-options", "nosniff"))
                    .add(("x-frame-options", "DENY"))
                    .add(("referrer-policy", "no-referrer"))
                    .add(("permissions-policy", "camera=(), microphone=(), geolocation=()")),
            )
            .wrap(middleware::Compress::default())
            .wrap(middleware::NormalizePath::trim())
            .wrap(middleware::Logger::default())
            .configure(api::configure)
            .default_service(web::to(web_assets::serve))
    })
    .bind(&runtime.bind)?
    .client_request_timeout(std::time::Duration::from_secs(15))
    .client_disconnect_timeout(std::time::Duration::from_secs(10))
    .keep_alive(std::time::Duration::from_secs(75))
    .max_connections(25_000)
    .run()
    .await
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("sonde=info,actix_web=info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}
