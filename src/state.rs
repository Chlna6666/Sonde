use std::sync::Arc;

use sea_orm::DatabaseConnection;
use tokio::sync::{Mutex, RwLock, broadcast};

use crate::{
    config::{InstallationConfig, RuntimeConfig},
    database,
    error::AppError,
    security::AuthSecurity,
};

pub struct InstalledState {
    pub database: DatabaseConnection,
    pub config: InstallationConfig,
    pub auth_security: Arc<AuthSecurity>,
}

pub struct AppState {
    pub runtime: RuntimeConfig,
    installed: RwLock<Option<Arc<InstalledState>>>,
    pub setup_lock: Mutex<()>,
    pub live_updates: broadcast::Sender<String>,
}

impl AppState {
    pub async fn load(runtime: RuntimeConfig) -> Result<Self, AppError> {
        let installed = if runtime.config_path.exists() {
            let mut config =
                InstallationConfig::read(&runtime.config_path).map_err(|_| AppError::Internal)?;
            if let Some(url) = &runtime.database_url_override {
                config.database_url.clone_from(url);
            }
            let database = database::connect(&config.database_url).await?;
            database::migrate(&database).await?;
            let auth_security = AuthSecurity::new(runtime.password_pepper.as_bytes())?;
            Some(Arc::new(InstalledState {
                database,
                config,
                auth_security: Arc::new(auth_security),
            }))
        } else if let Some(url) = &runtime.database_url_override {
            match database::connect(url).await {
                Ok(database) => {
                    let _ = database::migrate(&database).await;
                    if is_database_installed(&database).await {
                        let config = InstallationConfig {
                            database_url: url.clone(),
                            locale: "en".into(),
                            timezone: "UTC".into(),
                            secure_cookie: false,
                        };
                        let _ = config.write_atomic(&runtime.config_path);
                        let auth_security = AuthSecurity::new(runtime.password_pepper.as_bytes())?;
                        Some(Arc::new(InstalledState {
                            database,
                            config,
                            auth_security: Arc::new(auth_security),
                        }))
                    } else {
                        None
                    }
                }
                Err(_) => None,
            }
        } else {
            let default_sqlite_path = runtime.data_dir.join("sonde.sqlite");
            if default_sqlite_path.exists() {
                let path_str = default_sqlite_path.to_string_lossy().replace('\\', "/");
                let db_url = format!("sqlite://{path_str}?mode=rwc");
                match database::connect(&db_url).await {
                    Ok(database) => {
                        let _ = database::migrate(&database).await;
                        if is_database_installed(&database).await {
                            let config = InstallationConfig {
                                database_url: db_url,
                                locale: "en".into(),
                                timezone: "UTC".into(),
                                secure_cookie: false,
                            };
                            let _ = config.write_atomic(&runtime.config_path);
                            let auth_security =
                                AuthSecurity::new(runtime.password_pepper.as_bytes())?;
                            Some(Arc::new(InstalledState {
                                database,
                                config,
                                auth_security: Arc::new(auth_security),
                            }))
                        } else {
                            None
                        }
                    }
                    Err(_) => None,
                }
            } else {
                None
            }
        };
        let (live_updates, _) = broadcast::channel(256);
        Ok(Self {
            runtime,
            installed: RwLock::new(installed),
            setup_lock: Mutex::new(()),
            live_updates,
        })
    }

    pub async fn installed(&self) -> Result<Arc<InstalledState>, AppError> {
        self.installed
            .read()
            .await
            .clone()
            .ok_or(AppError::NotInitialized)
    }

    pub async fn is_installed(&self) -> bool {
        self.installed.read().await.is_some()
    }

    
    pub async fn update_installed_config<F>(&self, update_fn: F) -> Result<InstallationConfig, AppError>
    where
        F: FnOnce(&mut InstallationConfig),
    {
        let mut guard = self.installed.write().await;
        if let Some(installed_ref) = guard.as_ref() {
            let mut new_config = installed_ref.config.clone();
            update_fn(&mut new_config);
            new_config
                .write_atomic(&self.runtime.config_path)
                .map_err(|_| AppError::Internal)?;
            *guard = Some(Arc::new(InstalledState {
                database: installed_ref.database.clone(),
                config: new_config.clone(),
                auth_security: installed_ref.auth_security.clone(),
            }));
            Ok(new_config)
        } else {
            Err(AppError::NotInitialized)
        }
    }

    pub async fn finish_setup(&self, state: InstalledState) -> Result<(), AppError> {
        let mut guard = self.installed.write().await;
        if guard.is_some() {
            return Err(AppError::AlreadyInitialized);
        }
        crate::services::retention::spawn_retention_worker(state.database.clone());
        crate::services::alerts::spawn_alert_evaluator_worker(state.database.clone());
        *guard = Some(Arc::new(state));
        Ok(())
    }
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
            .map(|v| v == "true")
            .unwrap_or(false),
        _ => false,
    }
}
