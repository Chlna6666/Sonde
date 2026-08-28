use std::sync::Arc;

use crate::{
    auth,
    config::InstallationConfig,
    database,
    database::auth_repo,
    error::AppError,
    security::AuthSecurity,
    state::{AppState, InstalledState},
};
use std::{fs, path::Path};

pub struct SetupInput<'a> {
    pub database_type: &'a str,
    pub database_url: Option<&'a str>,
    pub locale: &'a str,
    pub timezone: &'a str,
    pub email: &'a str,
    pub username: &'a str,
    pub password: &'a str,
    pub secure_cookie: bool,
}

pub async fn test_connection(
    state: &AppState,
    database_type: &str,
    database_url: Option<&str>,
) -> Result<(), AppError> {
    let database_url = resolve_database_url(state, database_type, database_url)?;
    prepare_sqlite_path(&database_url)?;
    let database = database::connect(&database_url).await?;
    database.ping().await?;
    database.close().await?;
    Ok(())
}

pub async fn complete(state: &AppState, input: SetupInput<'_>) -> Result<(), AppError> {
    let database_url = resolve_database_url(state, input.database_type, input.database_url)?;
    validate_identity(input.email, input.username)?;
    auth::validate_password(input.password)?;
    prepare_sqlite_path(&database_url)?;
    let database = database::connect(&database_url).await?;
    database::migrate(&database).await?;
    let password = input.password.to_owned();
    let pepper = state.runtime.password_pepper.as_bytes().to_vec();
    let password_hash =
        tokio::task::spawn_blocking(move || auth::hash_password(&password, &pepper))
            .await
            .map_err(|_| AppError::Internal)??;
    auth_repo::create_super_admin(
        &database,
        input.email,
        input.username,
        &password_hash,
        input.locale,
    )
    .await?;
    let config = InstallationConfig {
        database_url,
        locale: input.locale.to_owned(),
        timezone: input.timezone.to_owned(),
        secure_cookie: input.secure_cookie,
    };
    config
        .write_atomic(&state.runtime.config_path)
        .map_err(|_| AppError::Internal)?;
    let auth_security = Arc::new(AuthSecurity::new(state.runtime.password_pepper.as_bytes())?);
    state
        .finish_setup(InstalledState::new(database, config, auth_security))
        .await
}

fn resolve_database_url(
    state: &AppState,
    database_type: &str,
    supplied_url: Option<&str>,
) -> Result<String, AppError> {
    match database_type {
        "sqlite" => {
            if let Some(url) = supplied_url.filter(|value| !value.trim().is_empty()) {
                validate_remote_url(Some(url), &["sqlite:"])
            } else {
                let path = state
                    .runtime
                    .data_dir
                    .join("sonde.sqlite")
                    .to_string_lossy()
                    .replace('\\', "/");
                Ok(format!("sqlite://{path}?mode=rwc"))
            }
        }
        "postgresql" => validate_remote_url(supplied_url, &["postgres:", "postgresql:"]),
        "mysql" => validate_remote_url(supplied_url, &["mysql:"]),
        _ => Err(AppError::Validation("unsupported database type".into())),
    }
}

fn validate_remote_url(url: Option<&str>, accepted: &[&str]) -> Result<String, AppError> {
    let url = url
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            AppError::Validation("database URL is required for a remote database".into())
        })?;
    if !accepted.iter().any(|prefix| url.starts_with(prefix)) {
        return Err(AppError::Validation(
            "database URL does not match the selected database type".into(),
        ));
    }
    Ok(url.to_owned())
}

fn prepare_sqlite_path(database_url: &str) -> Result<(), AppError> {
    let raw_path = if let Some(stripped) = database_url.strip_prefix("sqlite:///") {
        stripped
    } else if let Some(stripped) = database_url.strip_prefix("sqlite://") {
        stripped
    } else if let Some(stripped) = database_url.strip_prefix("sqlite:") {
        stripped
    } else {
        return Ok(());
    };
    if raw_path.starts_with(":memory:") || raw_path.is_empty() {
        return Ok(());
    }
    let path_text = raw_path.split('?').next().unwrap_or_default();
    if path_text.contains("..") || path_text.contains('\0') {
        return Err(AppError::Validation(
            "SQLite database path cannot contain traversal sequences or null bytes".into(),
        ));
    }
    let path = Path::new(path_text);
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|_| AppError::Internal)?;
    }
    Ok(())
}

fn validate_identity(email: &str, username: &str) -> Result<(), AppError> {
    if !email.contains('@') || email.len() > 254 {
        return Err(AppError::Validation(
            "valid super administrator email is required".into(),
        ));
    }
    if username.len() < 2 || username.len() > 64 {
        return Err(AppError::Validation(
            "username must be 2..64 characters".into(),
        ));
    }
    Ok(())
}
