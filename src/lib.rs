//! Self-hosted telemetry analytics for multiple applications.
//!
//! The `sonde` binary installs Microsoft mimalloc v3 as the process global allocator
//! so the Actix HTTP stack, ingest queues, and JSON codecs share a fragmentation-aware
//! heap. Library consumers should set their own allocator if they embed these modules.

mod bootstrap;

pub mod api;
pub mod auth;
pub mod config;
pub mod database;
pub mod domain;
pub mod error;
pub mod ingest_signature;
pub mod json;
pub mod security;
pub mod services;
pub mod state;
pub mod totp;
pub mod web_assets;

use std::{io, sync::Arc};

use actix_web::{App, HttpServer, middleware, web};
use tracing::{info, warn};
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

    if !state.is_installed().await {
        info!("Sonde is waiting for one-time web setup");
    }
    if !runtime.bind_is_loopback() && runtime.allow_insecure_cookies {
        warn!(
            "SONDE_ALLOW_INSECURE_COOKIES is set: session cookies will not be marked Secure on a \
             non-loopback bind; side effects include cookie theft over plaintext HTTP"
        );
    }
    if runtime.bind_is_loopback() {
        info!(
            "SONDE_BIND is loopback-only; remote clients must reach Sonde through a reverse proxy"
        );
    }
    info!(address = %runtime.bind, "starting Sonde");
    let emit_hsts = !runtime.bind_is_loopback();

    HttpServer::new(move || {
        let mut headers = middleware::DefaultHeaders::new()
            .add(("content-security-policy", "default-src 'self'; script-src 'self'; style-src 'self'; font-src 'self' data:; img-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'self'; frame-ancestors 'none'; form-action 'self'"))
            .add(("x-content-type-options", "nosniff"))
            .add(("x-frame-options", "DENY"))
            .add(("referrer-policy", "no-referrer"))
            .add(("permissions-policy", "camera=(), microphone=(), geolocation=()"));
        if emit_hsts {
            headers = headers.add((
                "strict-transport-security",
                "max-age=31536000; includeSubDomains",
            ));
        }
        App::new()
            .app_data(web::Data::new(state.clone()))
            .app_data(api::json_config(2_097_152))
            .app_data(api::query_config())
            .app_data(api::path_config())
            .app_data(web::PayloadConfig::default().limit(4_194_304))
            .wrap(headers)
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
