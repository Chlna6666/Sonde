use std::sync::Arc;

use sea_orm::DatabaseConnection;

use crate::{
    config::{InstallationConfig, RuntimeConfig},
    database,
    error::AppError,
    security::AuthSecurity,
    state::InstalledState,
};

pub async fn load_installed(
    runtime: &RuntimeConfig,
) -> Result<Option<Arc<InstalledState>>, AppError> {
    if runtime.config_path.exists() {
        return load_from_config(runtime).await.map(Some);
    }

    if let Some(url) = runtime.database_url_override.as_deref() {
        return recover_existing_database(runtime, url).await;
    }

    let default_sqlite_path = runtime.data_dir.join("sonde.sqlite");
    if !default_sqlite_path.exists() {
        return Ok(None);
    }

    let path = default_sqlite_path.to_string_lossy().replace('\\', "/");
    let database_url = format!("sqlite://{path}?mode=rwc");
    recover_existing_database(runtime, &database_url).await
}

async fn load_from_config(runtime: &RuntimeConfig) -> Result<Arc<InstalledState>, AppError> {
    let mut config = InstallationConfig::read(&runtime.config_path)
        .map_err(|error| AppError::internal("read installation config", error))?;
    if let Some(url) = &runtime.database_url_override {
        config.database_url.clone_from(url);
    }

    let database = database::connect(&config.database_url).await?;
    database::migrate(&database).await?;
    build_installed(runtime, database, config)
}

async fn recover_existing_database(
    runtime: &RuntimeConfig,
    database_url: &str,
) -> Result<Option<Arc<InstalledState>>, AppError> {
    let database = match database::connect(database_url).await {
        Ok(database) => database,
        Err(_) => return Ok(None),
    };
    database::migrate(&database).await?;
    if !is_database_installed(&database).await {
        return Ok(None);
    }

    let config = InstallationConfig {
        database_url: database_url.to_owned(),
        locale: "en".into(),
        timezone: "UTC".into(),
        secure_cookie: !runtime.bind_is_loopback(),
    };
    config
        .write_atomic(&runtime.config_path)
        .map_err(|error| AppError::internal("write recovered installation config", error))?;
    build_installed(runtime, database, config).map(Some)
}

fn build_installed(
    runtime: &RuntimeConfig,
    database: DatabaseConnection,
    config: InstallationConfig,
) -> Result<Arc<InstalledState>, AppError> {
    let auth_security = Arc::new(AuthSecurity::new(runtime.password_pepper.as_bytes())?);
    Ok(Arc::new(InstalledState::new(
        database,
        config,
        auth_security,
    )))
}

pub(crate) fn spawn_background_workers(state: &InstalledState) {
    crate::services::workers::spawn_leased_workers(state.database.clone());
    // Rollups use generation-based contention control and are intentionally safe to run on every
    // replica; distributing dirty-day work avoids making the aggregation pipeline leader-bound.
    crate::services::rollups::spawn_rollup_worker(state.database.clone());
}

async fn is_database_installed(database: &DatabaseConnection) -> bool {
    use sea_orm::{
        ConnectionTrait,
        sea_query::{Alias, Expr, ExprTrait, Query},
    };

    let query = Query::select()
        .column(Alias::new("value"))
        .from(Alias::new("system_state"))
        .and_where(Expr::col(Alias::new("key")).eq("installed"))
        .limit(1)
        .to_owned();
    match database.query_one(&query).await {
        Ok(Some(row)) => row
            .try_get::<String>("", "value")
            .map(|value| value == "true")
            .unwrap_or(false),
        _ => false,
    }
}
