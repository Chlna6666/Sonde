use sha2::{Digest, Sha256};

use crate::{
    auth,
    database::{application_delete, applications as application_store},
    error::AppError,
    services::authentication::AuthenticatedUser,
    state::InstalledState,
};

pub use super::application_models::{
    ApiKeyDetail, AppMemberSummary, ApplicationSummary, EnvironmentSummary, PublicApplicationInfo,
    UpdateApplicationParams,
};

pub async fn list(
    installed: &InstalledState,
    user: &AuthenticatedUser,
) -> Result<Vec<ApplicationSummary>, AppError> {
    let is_admin = user
        .roles
        .iter()
        .any(|r| r == "Super Admin" || r == "Admin")
        || user.grants.iter().any(|g| g.allows("*", None));
    let records =
        application_store::list_applications(&installed.database, Some(&user.id), is_admin).await?;
    Ok(records.into_iter().map(map_application).collect())
}

pub async fn public_by_slug(
    installed: &InstalledState,
    slug: &str,
) -> Result<Option<PublicApplicationInfo>, AppError> {
    Ok(application_store::get_public_application_by_slug(&installed.database, slug)
        .await?
        .map(map_public_application))
}

pub async fn list_environments(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    application_id: &str,
) -> Result<Vec<EnvironmentSummary>, AppError> {
    ensure_app_access(&installed.database, user, application_id, false).await?;
    let records = application_store::list_environments(&installed.database, application_id).await?;
    Ok(records.into_iter().map(map_environment).collect())
}

pub async fn create(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    name: &str,
    slug: &str,
) -> Result<(String, String), AppError> {
    validate_application(name, slug)?;
    let created =
        application_store::create_application(&installed.database, name, slug, Some(&user.id)).await?;
    application_store::audit(
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
) -> Result<Vec<ApiKeyDetail>, AppError> {
    ensure_app_access(&installed.database, user, application_id, false).await?;
    let records = application_store::list_api_keys(&installed.database, application_id).await?;
    Ok(records.into_iter().map(map_api_key).collect())
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
    let id = application_store::create_api_key(
        &installed.database,
        application_id,
        environment_id,
        name,
        &hash,
        &prefix,
        scopes,
    )
    .await?;
    application_store::audit(
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
    let revoked = application_store::revoke_api_key(&installed.database, application_id, key_id).await?;
    if !revoked {
        return Err(AppError::NotFound);
    }
    application_store::audit(
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
    let deleted = application_store::delete_api_key(&installed.database, application_id, key_id).await?;
    if !deleted {
        return Err(AppError::NotFound);
    }
    application_store::audit(
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
    let count = application_store::delete_revoked_api_keys(&installed.database, application_id).await?;
    application_store::audit(
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
    let old_key = application_store::get_api_key(&installed.database, application_id, key_id)
        .await?
        .ok_or(AppError::NotFound)?;

    let _ = application_store::revoke_api_key(&installed.database, application_id, key_id).await?;

    let raw_key = format!("sonde_{}", auth::random_token(32));
    let prefix = raw_key.chars().take(12).collect::<String>();
    let hash = hex::encode(Sha256::digest(raw_key.as_bytes()));
    let new_id = application_store::create_api_key(
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

    application_store::audit(
        &installed.database,
        Some(&user.id),
        "api_key.regenerated",
        "api_key",
        Some(&new_id),
    )
    .await?;
    Ok(raw_key)
}

pub async fn update(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    application_id: &str,
    params: UpdateApplicationParams<'_>,
) -> Result<(), AppError> {
    ensure_app_access(&installed.database, user, application_id, true).await?;
    validate_application(params.name, params.slug)?;
    application_store::update_application(
        &installed.database,
        application_id,
        application_store::UpdateApplicationParams {
            name: params.name,
            slug: params.slug,
            retention_days: params.retention_days.clamp(0, 36500),
            is_public: params.is_public,
            description: params.description,
            github_url: params.github_url,
            website_url: params.website_url,
            custom_header: params.custom_header,
        },
    )
    .await?;
    application_store::audit(
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
    application_delete::delete_application_exact(&installed.database, application_id).await?;
    application_store::audit(
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
) -> Result<Vec<AppMemberSummary>, AppError> {
    ensure_app_access(&installed.database, user, application_id, false).await?;
    let records = application_store::list_application_members(&installed.database, application_id).await?;
    Ok(records.into_iter().map(map_member).collect())
}

pub async fn grant_member(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    application_id: &str,
    target_user_id: &str,
    role: &str,
) -> Result<(), AppError> {
    ensure_app_access(&installed.database, user, application_id, true).await?;
    application_store::grant_application_access(&installed.database, application_id, target_user_id, role)
        .await?;
    application_store::audit(
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
    application_store::revoke_application_access(&installed.database, application_id, target_user_id)
        .await?;
    application_store::audit(
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
    let app = application_store::get_application(database, application_id)
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

fn map_application(record: application_store::ApplicationSummary) -> ApplicationSummary {
    ApplicationSummary {
        id: record.id,
        name: record.name,
        slug: record.slug,
        retention_days: record.retention_days,
        owner_user_id: record.owner_user_id,
        is_public: record.is_public,
        description: record.description,
        github_url: record.github_url,
        website_url: record.website_url,
        custom_header: record.custom_header,
        created_at: record.created_at,
    }
}

fn map_public_application(record: application_store::PublicApplicationInfo) -> PublicApplicationInfo {
    PublicApplicationInfo {
        id: record.id,
        name: record.name,
        slug: record.slug,
        is_public: record.is_public,
        description: record.description,
        github_url: record.github_url,
        website_url: record.website_url,
        custom_header: record.custom_header,
        created_at: record.created_at,
    }
}

fn map_environment(record: application_store::EnvironmentSummary) -> EnvironmentSummary {
    EnvironmentSummary {
        id: record.id,
        application_id: record.application_id,
        name: record.name,
        slug: record.slug,
    }
}

fn map_member(record: application_store::AppMemberSummary) -> AppMemberSummary {
    AppMemberSummary {
        user_id: record.user_id,
        username: record.username,
        email: record.email,
        role: record.role,
        granted_at: record.granted_at,
    }
}

fn map_api_key(record: application_store::ApiKeyDetail) -> ApiKeyDetail {
    ApiKeyDetail {
        id: record.id,
        application_id: record.application_id,
        environment_id: record.environment_id,
        environment_name: record.environment_name,
        name: record.name,
        key_prefix: record.key_prefix,
        scopes: record.scopes,
        expires_at: record.expires_at,
        last_used_at: record.last_used_at,
        revoked_at: record.revoked_at,
        created_at: record.created_at,
        is_active: record.is_active,
    }
}
