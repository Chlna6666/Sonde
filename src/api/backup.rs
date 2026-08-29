use actix_web::{HttpRequest, HttpResponse, Responder, error as web_error, web};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;

use crate::{
    database::{backup_v2_repo, legacy_backup_repo},
    error::AppError,
    services::{authentication, backup},
    state::AppState,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemSettingsResponse {
    pub timezone: String,
    pub locale: String,
    pub secure_cookie: bool,
    pub server_time: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSystemSettingsRequest {
    pub timezone: Option<String>,
    pub locale: Option<String>,
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route(
        "/api/v1/admin/applications/{application_id}/export",
        web::get().to(export_application),
    )
    .route(
        "/api/v1/admin/applications/import",
        web::post().to(import_application),
    )
    .route(
        "/api/v1/admin/system/backup",
        web::get().to(export_system_backup),
    )
    .route(
        "/api/v1/admin/system/restore",
        web::post().to(restore_system_backup),
    )
    .route(
        "/api/v1/admin/system/backup/v2",
        web::get().to(export_system_backup_v2),
    )
    .route(
        "/api/v1/admin/system/restore/v2",
        web::post().to(restore_system_backup_v2),
    )
    .route(
        "/api/v1/admin/system/settings",
        web::get().to(get_system_settings),
    )
    .route(
        "/api/v1/admin/system/settings",
        web::patch().to(update_system_settings),
    );
}

async fn get_system_settings(
    state: web::Data<Arc<AppState>>,
    req: HttpRequest,
) -> Result<impl Responder, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate(&installed, &req).await?;
    user.require("settings.manage", None)?;
    let now = chrono::Utc::now().timestamp_millis();
    Ok(HttpResponse::Ok().json(SystemSettingsResponse {
        timezone: installed.config.timezone.clone(),
        locale: installed.config.locale.clone(),
        secure_cookie: installed.config.secure_cookie,
        server_time: now,
    }))
}

async fn update_system_settings(
    state: web::Data<Arc<AppState>>,
    req: HttpRequest,
    body: web::Json<UpdateSystemSettingsRequest>,
) -> Result<impl Responder, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate_mutation(&installed, &req).await?;
    user.require("settings.manage", None)?;

    let payload = body.into_inner();
    let updated = state.get_ref().as_ref()
        .update_installed_config(|cfg| {
            if let Some(tz) = payload.timezone {
                if !tz.trim().is_empty() {
                    cfg.timezone = tz.trim().to_string();
                }
            }
            if let Some(loc) = payload.locale {
                if !loc.trim().is_empty() {
                    cfg.locale = loc.trim().to_string();
                }
            }
        })
        .await?;

    let now = chrono::Utc::now().timestamp_millis();
    Ok(HttpResponse::Ok().json(SystemSettingsResponse {
        timezone: updated.timezone,
        locale: updated.locale,
        secure_cookie: updated.secure_cookie,
        server_time: now,
    }))
}

async fn export_application(
    state: web::Data<Arc<AppState>>,
    req: HttpRequest,
    path: web::Path<String>,
) -> Result<impl Responder, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate(&installed, &req).await?;
    let app_id = path.into_inner();
    let export_data = backup::export_application(&installed, &user, &app_id).await?;
    Ok(HttpResponse::Ok().json(export_data))
}

async fn import_application(
    state: web::Data<Arc<AppState>>,
    req: HttpRequest,
    body: web::Json<legacy_backup_repo::SingleAppExport>,
) -> Result<impl Responder, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate_mutation(&installed, &req).await?;
    let new_app_id = backup::import_application(&installed, &user, body.into_inner()).await?;
    Ok(HttpResponse::Created().json(serde_json::json!({
        "ok": true,
        "applicationId": new_app_id
    })))
}

async fn export_system_backup(
    state: web::Data<Arc<AppState>>,
    req: HttpRequest,
) -> Result<impl Responder, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate(&installed, &req).await?;
    let backup_data = backup::export_full_system(&installed, &user).await?;
    Ok(HttpResponse::Ok().json(backup_data))
}

async fn restore_system_backup(
    state: web::Data<Arc<AppState>>,
    req: HttpRequest,
    body: web::Json<legacy_backup_repo::FullSystemBackup>,
) -> Result<impl Responder, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate_mutation(&installed, &req).await?;
    backup::restore_full_system(&installed, &user, body.into_inner()).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "ok": true
    })))
}

async fn export_system_backup_v2(
    state: web::Data<Arc<AppState>>,
    req: HttpRequest,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate(&installed, &req).await?;
    let stream = backup::export_full_system_v2(&installed, &user).await?;
    let body = stream.map(|chunk| {
        chunk
            .map(web::Bytes::from)
            .map_err(web_error::ErrorInternalServerError)
    });
    let date = chrono::Utc::now().format("%Y-%m-%d");

    Ok(HttpResponse::Ok()
        .insert_header(("content-type", backup_v2_repo::CONTENT_TYPE))
        .insert_header(("cache-control", "no-store"))
        .insert_header((
            "content-disposition",
            format!(
                "attachment; filename=\"sonde-full-backup-{date}.{}\"",
                backup_v2_repo::FILE_EXTENSION
            ),
        ))
        .streaming(body))
}

async fn restore_system_backup_v2(
    state: web::Data<Arc<AppState>>,
    req: HttpRequest,
    mut body: web::Payload,
) -> Result<HttpResponse, AppError> {
    let installed = state.installed().await?;
    let user = authentication::authenticate_mutation(&installed, &req).await?;

    // NamedTempFile owns deletion. The async handle returned by reopen writes to the same inode,
    // while the guard keeps the path alive for the validation and restore passes.
    let staging = tempfile::NamedTempFile::new().map_err(|_| AppError::Internal)?;
    let staging_file = staging.reopen().map_err(|_| AppError::Internal)?;
    let mut staging_file = tokio::fs::File::from_std(staging_file);
    let mut current_record_bytes = 0_usize;

    while let Some(chunk) = body.next().await {
        let chunk = chunk
            .map_err(|_| AppError::Validation("backup upload was interrupted".into()))?;
        for byte in chunk.as_ref() {
            if *byte == b'\n' {
                current_record_bytes = 0;
            } else {
                current_record_bytes = current_record_bytes.saturating_add(1);
                if current_record_bytes > backup_v2_repo::MAX_RECORD_BYTES {
                    return Err(AppError::PayloadTooLarge);
                }
            }
        }
        staging_file
            .write_all(&chunk)
            .await
            .map_err(|_| AppError::Internal)?;
    }
    staging_file
        .flush()
        .await
        .map_err(|_| AppError::Internal)?;
    staging_file
        .sync_all()
        .await
        .map_err(|_| AppError::Internal)?;
    drop(staging_file);

    let restored = backup::restore_full_system_v2(&installed, &user, staging.path()).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "ok": true,
        "restoredRecords": restored,
        "formatVersion": backup_v2_repo::FORMAT_VERSION,
    })))
}
