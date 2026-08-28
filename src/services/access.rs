use crate::{
    auth, database::auth_repo, error::AppError, services::authentication::AuthenticatedUser,
    state::InstalledState,
};

pub async fn list_users(
    installed: &InstalledState,
    user: &AuthenticatedUser,
) -> Result<Vec<auth_repo::UserSummary>, AppError> {
    user.require("members.read", None)?;
    Ok(auth_repo::list_users(&installed.database).await?)
}

pub async fn list_audit_logs(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    page: u64,
    page_size: u64,
    action: Option<&str>,
    resource_type: Option<&str>,
) -> Result<crate::database::app_repo::AuditLogPage, AppError> {
    user.require("audit.read", None)?;
    let page = page.max(1);
    let page_size = page_size.clamp(1, 100);
    Ok(crate::database::app_repo::list_audit_logs(
        &installed.database,
        page,
        page_size,
        action,
        resource_type,
    )
    .await?)
}

pub struct CreateUserInput<'a> {
    pub email: &'a str,
    pub username: &'a str,
    pub password: &'a str,
    pub locale: &'a str,
    pub role: &'a str,
    pub pepper: &'a [u8],
}

pub async fn create_user(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    input: CreateUserInput<'_>,
) -> Result<String, AppError> {
    user.require("members.manage", None)?;
    if input.email.trim().is_empty() || input.username.trim().is_empty() || input.password.len() < 8
    {
        return Err(AppError::Validation(
            "email, username and password (>= 8 chars) are required".into(),
        ));
    }
    let password_hash = auth::hash_password(input.password, input.pepper)?;
    let user_id = auth_repo::create_user(
        &installed.database,
        input.email,
        input.username,
        &password_hash,
        if input.locale.is_empty() {
            "en"
        } else {
            input.locale
        },
        input.role,
    )
    .await?;

    crate::database::app_repo::audit(
        &installed.database,
        Some(&user.id),
        "user.created",
        "user",
        Some(&user_id),
    )
    .await?;

    Ok(user_id)
}

pub struct UpdateUserInput<'a> {
    pub email: &'a str,
    pub username: &'a str,
    pub locale: &'a str,
    pub active: bool,
    pub role: Option<&'a str>,
}

pub async fn update_user(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    target_user_id: &str,
    input: UpdateUserInput<'_>,
) -> Result<(), AppError> {
    user.require("members.manage", None)?;
    if input.email.trim().is_empty() || input.username.trim().is_empty() {
        return Err(AppError::Validation(
            "email and username are required".into(),
        ));
    }
    auth_repo::update_user(
        &installed.database,
        target_user_id,
        input.email,
        input.username,
        input.locale,
        input.active,
        input.role,
    )
    .await?;

    crate::database::app_repo::audit(
        &installed.database,
        Some(&user.id),
        "user.updated",
        "user",
        Some(target_user_id),
    )
    .await?;

    Ok(())
}

pub async fn reset_password(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    target_user_id: &str,
    new_password: &str,
    pepper: &[u8],
) -> Result<(), AppError> {
    user.require("members.manage", None)?;
    if new_password.len() < 8 {
        return Err(AppError::Validation(
            "password must be at least 8 characters".into(),
        ));
    }
    let password_hash = auth::hash_password(new_password, pepper)?;
    auth_repo::update_password_hash(&installed.database, target_user_id, &password_hash).await?;

    crate::database::app_repo::audit(
        &installed.database,
        Some(&user.id),
        "user.password_reset",
        "user",
        Some(target_user_id),
    )
    .await?;

    Ok(())
}

pub async fn delete_user(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    target_user_id: &str,
) -> Result<(), AppError> {
    user.require("members.manage", None)?;
    if user.id == target_user_id {
        return Err(AppError::Validation(
            "cannot delete current user account".into(),
        ));
    }
    auth_repo::delete_user(&installed.database, target_user_id).await?;

    crate::database::app_repo::audit(
        &installed.database,
        Some(&user.id),
        "user.deleted",
        "user",
        Some(target_user_id),
    )
    .await?;

    Ok(())
}

pub async fn list_roles(
    installed: &InstalledState,
    user: &AuthenticatedUser,
) -> Result<Vec<auth_repo::RoleSummary>, AppError> {
    user.require("members.read", None)?;
    Ok(auth_repo::list_roles(&installed.database).await?)
}

pub async fn get_user_assigned_applications(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    target_user_id: &str,
) -> Result<Vec<String>, AppError> {
    user.require("members.read", None)?;
    Ok(crate::database::app_repo::get_user_assigned_applications(
        &installed.database,
        target_user_id,
    )
    .await?)
}

pub async fn set_user_assigned_applications(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    target_user_id: &str,
    app_ids: &[String],
    role: &str,
) -> Result<(), AppError> {
    user.require("members.manage", None)?;
    crate::database::app_repo::set_user_assigned_applications(
        &installed.database,
        target_user_id,
        app_ids,
        role,
    )
    .await?;

    crate::database::app_repo::audit(
        &installed.database,
        Some(&user.id),
        "user.applications_assigned",
        "user",
        Some(target_user_id),
    )
    .await?;

    Ok(())
}
