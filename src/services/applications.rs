use sha2::{Digest, Sha256};

use crate::{
    auth, database::app_repo, error::AppError, services::authentication::AuthenticatedUser,
    state::InstalledState,
};

pub async fn list(
    installed: &InstalledState,
    user: &AuthenticatedUser,
) -> Result<Vec<app_repo::ApplicationSummary>, AppError> {
    let is_admin = user
        .roles
        .iter()
        .any(|r| r == "Super Admin" || r == "Admin")
        || user.grants.iter().any(|g| g.allows("*", None));
    Ok(app_repo::list_applications(&installed.database, Some(&user.id), is_admin).await?)
}

pub async fn list_environments(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    application_id: &str,
) -> Result<Vec<app_repo::EnvironmentSummary>, AppError> {
    ensure_app_access(&installed.database, user, application_id, false).await?;
    Ok(app_repo::list_environments(&installed.database, application_id).await?)
}

pub async fn create(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    name: &str,
    slug: &str,
) -> Result<(String, String), AppError> {
    validate_application(name, slug)?;
    let created =
        app_repo::create_application(&installed.database, name, slug, Some(&user.id)).await?;
    app_repo::audit(
        &installed.database,
        Some(&user.id),
        "application.created",
        "application",
        Some(&created.0),
    )
    .await?;
    Ok(created)
}

pub async fn list_keys(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    application_id: &str,
) -> Result<Vec<app_repo::ApiKeyDetail>, AppError> {
    ensure_app_access(&installed.database, user, application_id, false).await?;
    Ok(app_repo::list_api_keys(&installed.database, application_id).await?)
}

pub async fn create_key(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    application_id: &str,
    environment_id: &str,
    name: &str,
    scopes: &[String],
) -> Result<String, AppError> {
    ensure_app_access(&installed.database, user, application_id, true).await?;
    if name.trim().is_empty() || scopes.is_empty() {
        return Err(AppError::Validation(
            "key name and scopes are required".into(),
        ));
    }
    let raw_key = format!("sonde_{}", auth::random_token(32));
    let prefix = raw_key.chars().take(12).collect::<String>();
    let hash = hex::encode(Sha256::digest(raw_key.as_bytes()));
    let id = app_repo::create_api_key(
        &installed.database,
        application_id,
        environment_id,
        name,
        &hash,
        &prefix,
        scopes,
    )
    .await?;
    app_repo::audit(
        &installed.database,
        Some(&user.id),
        "api_key.created",
        "api_key",
        Some(&id),
    )
    .await?;
    Ok(raw_key)
}

pub async fn revoke_key(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    application_id: &str,
    key_id: &str,
) -> Result<(), AppError> {
    ensure_app_access(&installed.database, user, application_id, true).await?;
    let revoked = app_repo::revoke_api_key(&installed.database, application_id, key_id).await?;
    if !revoked {
        return Err(AppError::NotFound);
    }
    app_repo::audit(
        &installed.database,
        Some(&user.id),
        "api_key.revoked",
        "api_key",
        Some(key_id),
    )
    .await?;
    Ok(())
}

pub async fn delete_key(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    application_id: &str,
    key_id: &str,
) -> Result<(), AppError> {
    ensure_app_access(&installed.database, user, application_id, true).await?;
    let deleted = app_repo::delete_api_key(&installed.database, application_id, key_id).await?;
    if !deleted {
        return Err(AppError::NotFound);
    }
    app_repo::audit(
        &installed.database,
        Some(&user.id),
        "api_key.deleted",
        "api_key",
        Some(key_id),
    )
    .await?;
    Ok(())
}

pub async fn clear_revoked_keys(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    application_id: &str,
) -> Result<u64, AppError> {
    ensure_app_access(&installed.database, user, application_id, true).await?;
    let count = app_repo::delete_revoked_api_keys(&installed.database, application_id).await?;
    app_repo::audit(
        &installed.database,
        Some(&user.id),
        "api_key.cleared_revoked",
        "application",
        Some(application_id),
    )
    .await?;
    Ok(count)
}

pub async fn regenerate_key(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    application_id: &str,
    key_id: &str,
) -> Result<String, AppError> {
    ensure_app_access(&installed.database, user, application_id, true).await?;
    let old_key = app_repo::get_api_key(&installed.database, application_id, key_id)
        .await?
        .ok_or(AppError::NotFound)?;

    let _ = app_repo::revoke_api_key(&installed.database, application_id, key_id).await?;

    let raw_key = format!("sonde_{}", auth::random_token(32));
    let prefix = raw_key.chars().take(12).collect::<String>();
    let hash = hex::encode(Sha256::digest(raw_key.as_bytes()));
    let new_id = app_repo::create_api_key(
        &installed.database,
        application_id,
        &old_key.environment_id,
        &format!(
            "{} (Regenerated)",
            old_key.name.trim_end_matches(" (Regenerated)")
        ),
        &hash,
        &prefix,
        &old_key.scopes,
    )
    .await?;

    app_repo::audit(
        &installed.database,
        Some(&user.id),
        "api_key.regenerated",
        "api_key",
        Some(&new_id),
    )
    .await?;
    Ok(raw_key)
}

pub use crate::database::app_repo::UpdateApplicationParams;

pub async fn update(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    application_id: &str,
    mut params: UpdateApplicationParams<'_>,
) -> Result<(), AppError> {
    ensure_app_access(&installed.database, user, application_id, true).await?;
    validate_application(params.name, params.slug)?;
    params.retention_days = params.retention_days.clamp(0, 36500);
    app_repo::update_application(&installed.database, application_id, params).await?;
    app_repo::audit(
        &installed.database,
        Some(&user.id),
        "application.updated",
        "application",
        Some(application_id),
    )
    .await?;
    Ok(())
}

pub async fn delete(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    application_id: &str,
) -> Result<(), AppError> {
    ensure_app_access(&installed.database, user, application_id, true).await?;
    app_repo::delete_application(&installed.database, application_id).await?;
    app_repo::audit(
        &installed.database,
        Some(&user.id),
        "application.deleted",
        "application",
        Some(application_id),
    )
    .await?;
    Ok(())
}

pub async fn list_members(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    application_id: &str,
) -> Result<Vec<app_repo::AppMemberSummary>, AppError> {
    ensure_app_access(&installed.database, user, application_id, false).await?;
    Ok(app_repo::list_application_members(&installed.database, application_id).await?)
}

pub async fn grant_member(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    application_id: &str,
    target_user_id: &str,
    role: &str,
) -> Result<(), AppError> {
    ensure_app_access(&installed.database, user, application_id, true).await?;
    app_repo::grant_application_access(&installed.database, application_id, target_user_id, role)
        .await?;
    app_repo::audit(
        &installed.database,
        Some(&user.id),
        "application.member_granted",
        "application",
        Some(application_id),
    )
    .await?;
    Ok(())
}

pub async fn revoke_member(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    application_id: &str,
    target_user_id: &str,
) -> Result<(), AppError> {
    ensure_app_access(&installed.database, user, application_id, true).await?;
    app_repo::revoke_application_access(&installed.database, application_id, target_user_id)
        .await?;
    app_repo::audit(
        &installed.database,
        Some(&user.id),
        "application.member_revoked",
        "application",
        Some(application_id),
    )
    .await?;
    Ok(())
}

pub async fn ensure_app_access(
    database: &sea_orm::DatabaseConnection,
    user: &AuthenticatedUser,
    application_id: &str,
    write: bool,
) -> Result<(), AppError> {
    if user
        .roles
        .iter()
        .any(|r| r == "Super Admin" || r == "Admin")
        || user.grants.iter().any(|g| g.allows("*", None))
    {
        return Ok(());
    }
    let app = app_repo::get_application(database, application_id)
        .await?
        .ok_or(AppError::NotFound)?;
    if app.owner_user_id.as_deref() == Some(&user.id) {
        return Ok(());
    }
    if write {
        user.require("apps.manage", Some(application_id))?;
    } else {
        user.require("apps.read", Some(application_id))?;
    }
    Ok(())
}

fn validate_application(name: &str, slug: &str) -> Result<(), AppError> {
    if name.trim().is_empty() || name.len() > 80 {
        return Err(AppError::Validation(
            "application name must be 1..80 bytes".into(),
        ));
    }
    if slug.len() < 2
        || slug.len() > 64
        || !slug
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(AppError::Validation(
            "slug must contain lowercase letters, numbers, or hyphens".into(),
        ));
    }
    Ok(())
}
