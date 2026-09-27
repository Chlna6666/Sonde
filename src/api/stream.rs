use std::{convert::Infallible, sync::Arc, time::Duration};

use actix_web::{HttpRequest, HttpResponse, web};
use async_stream::stream;
use tokio::time::interval;

use crate::{error::AppError, services::authentication, state::AppState};

pub fn configure(config: &mut web::ServiceConfig) {
    config.route("/api/v1/admin/stream", web::get().to(updates));
}

async fn updates(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let mut user = authentication::authenticate(&installed, &request).await?;
    user.require("telemetry.read", None)?;
    let mut receiver = state.live_updates.subscribe();
    let mut keep_alive = interval(Duration::from_secs(15));
    let body = stream! {
        loop {
            tokio::select! {
                update = receiver.recv() => match update {
                    Ok(update) => {
                        // Live activity is scoped per application: a subscriber must not learn
                        // about applications it cannot read.
                        if user
                            .require("telemetry.read", Some(&update.application_id))
                            .is_err()
                        {
                            continue;
                        }
                        yield Ok::<_, Infallible>(web::Bytes::from(format!(
                            "event: telemetry\ndata: {}\n\n",
                            update.payload
                        )));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                },
                _ = keep_alive.tick() => {
                    // Long-lived streams must not outlive session expiry, account deactivation, or
                    // RBAC changes. Refresh the authorization snapshot before every keep-alive;
                    // failure closes the stream instead of continuing with stale grants.
                    match authentication::authenticate(&installed, &request).await {
                        Ok(refreshed)
                            if refreshed.require("telemetry.read", None).is_ok() =>
                        {
                            user = refreshed;
                            yield Ok(web::Bytes::from_static(b": keep-alive\n\n"));
                        }
                        _ => break,
                    }
                },
            }
        }
    };
    Ok(HttpResponse::Ok()
        .insert_header(("content-type", "text/event-stream"))
        .insert_header(("cache-control", "no-cache, no-store"))
        .streaming(body))
}
