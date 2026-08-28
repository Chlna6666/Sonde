use std::path::Path;

use futures_util::Stream;

use crate::{
    database::{app_repo, backup_repo, backup_v2_repo},
    error::AppError,
    services::{applications::ensure_app_access, authentication::AuthenticatedUser},
    state::InstalledState,
};

pub async fn export_application(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    application_id: &str,
) -> Result<backup_repo::SingleAppExport, AppError> {
    ensure_app_access(&installed.database, user, application_id, false).await?;
    let Some(export_data) =
        backup_repo::export_single_application(&installed.database, application_id).await?
    else {
        return Err(AppError::NotFound);
    };

    app_repo::audit(
        &installed.database,
        Some(&user.id),
        "application.exported",
        "application",
        Some(application_id),
    )
    .await?;

    Ok(export_data)
}

pub async fn import_application(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    payload: backup_repo::SingleAppExport,
) -> Result<String, AppError> {
    user.require("apps.manage", None)?;
    let new_app_id =
        backup_repo::import_single_application(&installed.database, Some(&user.id), payload)
            .await?;

    app_repo::audit(
        &installed.database,
        Some(&user.id),
        "application.imported",
        "application",
        Some(&new_app_id),
    )
    .await?;

    Ok(new_app_id)
}

pub async fn export_full_system(
    installed: &InstalledState,
    user: &AuthenticatedUser,
) -> Result<backup_repo::FullSystemBackup, AppError> {
    require_system_backup_access(user)?;

    let backup_data = backup_repo::export_full_system(&installed.database).await?;

    app_repo::audit(
        &installed.database,
        Some(&user.id),
        "system.backup_exported",
        "system",
        None,
    )
    .await?;

    Ok(backup_data)
}

pub async fn restore_full_system(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    payload: backup_repo::FullSystemBackup,
) -> Result<(), AppError> {
    require_system_backup_access(user)?;

    backup_repo::restore_full_system(&installed.database, payload).await?;

    app_repo::audit(
        &installed.database,
        Some(&user.id),
        "system.backup_restored",
        "system",
        None,
    )
    .await?;

    Ok(())
}

pub async fn export_full_system_v2(
    installed: &InstalledState,
    user: &AuthenticatedUser,
) -> Result<impl Stream<Item = Result<Vec<u8>, backup_v2_repo::BackupV2Error>>, AppError> {
    require_system_backup_access(user)?;

    app_repo::audit(
        &installed.database,
        Some(&user.id),
        "system.backup_export_requested",
        "system",
        None,
    )
    .await?;

    Ok(backup_v2_repo::export_full_system_stream(
        installed.database.clone(),
    ))
}

pub async fn restore_full_system_v2(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    path: &Path,
) -> Result<u64, AppError> {
    require_system_backup_access(user)?;

    let restored = backup_v2_repo::restore_full_system_from_file(&installed.database, path)
        .await
        .map_err(map_backup_v2_error)?;

    app_repo::audit(
        &installed.database,
        Some(&user.id),
        "system.backup_v2_restored",
        "system",
        None,
    )
    .await?;

    Ok(restored)
}

fn require_system_backup_access(user: &AuthenticatedUser) -> Result<(), AppError> {
    if user
        .roles
        .iter()
        .any(|role| role == "Super Admin" || role == "Admin")
        || user.grants.iter().any(|grant| grant.allows("*", None))
    {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

fn map_backup_v2_error(error: backup_v2_repo::BackupV2Error) -> AppError {
    match error {
        backup_v2_repo::BackupV2Error::Database(error) => AppError::Database(error),
        backup_v2_repo::BackupV2Error::Invalid(message) => AppError::Validation(message),
        backup_v2_repo::BackupV2Error::Json(error) => {
            AppError::Validation(format!("invalid backup JSON: {error}"))
        }
        backup_v2_repo::BackupV2Error::Io(_) => AppError::Internal,
    }
}
