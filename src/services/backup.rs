use crate::{
    database::{app_repo, backup_repo},
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
    if !user
        .roles
        .iter()
        .any(|r| r == "Super Admin" || r == "Admin")
        && !user.grants.iter().any(|g| g.allows("*", None))
    {
        return Err(AppError::Forbidden);
    }

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
    if !user
        .roles
        .iter()
        .any(|r| r == "Super Admin" || r == "Admin")
        && !user.grants.iter().any(|g| g.allows("*", None))
    {
        return Err(AppError::Forbidden);
    }

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
