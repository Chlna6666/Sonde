use std::sync::Arc;

use actix_web::{HttpRequest, HttpResponse, web};
use serde::Deserialize;

use crate::{
    error::AppError,
    services::{authentication, migrations},
    state::AppState,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreviewRequest {
    sql: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExecuteRequest {
    sql: String,
    application_id: String,
    environment_id: String,
}

pub fn configure(config: &mut web::ServiceConfig) {
    config.service(
        web::scope("/api/v1/admin/migrations")
            .route("/d1/preview", web::post().to(preview))
            .route("/d1/execute", web::post().to(execute))
            .route("/runs", web::get().to(list_runs)),
    );
}

async fn preview(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    body: web::Json<PreviewRequest>,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate_mutation(&installed, &request).await?;
    user.require("migrations.manage", None)?;
    Ok(HttpResponse::Ok().json(migrations::parse_d1_export(&body.sql)?.preview))
}

async fn execute(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
    body: web::Json<ExecuteRequest>,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate_mutation(&installed, &request).await?;
    user.require("migrations.manage", Some(&body.application_id))?;
    let result = migrations::execute_d1_import(
        &installed,
        &body.sql,
        &body.application_id,
        &body.environment_id,
    )
    .await?;
    Ok(HttpResponse::Ok().json(result))
}

async fn list_runs(
    state: web::Data<Arc<AppState>>,
    request: HttpRequest,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate(&installed, &request).await?;
    Ok(HttpResponse::Ok().json(migrations::list_runs(&installed, &user).await?))
}
