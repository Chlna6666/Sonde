use std::sync::Arc;

use sea_orm::DatabaseConnection;
use tokio::sync::{Mutex, OwnedSemaphorePermit, RwLock, Semaphore, broadcast};

use crate::{
    config::{InstallationConfig, RuntimeConfig},
    database,
    error::AppError,
    security::AuthSecurity,
    services::ingest_writer::IngestWriter,
};

const MAX_IN_FLIGHT_INGEST_REQUESTS: usize = 64;
const MAX_IN_FLIGHT_ANALYTICS_QUERIES: usize = 8;

pub struct InstalledState {
    pub database: DatabaseConnection,
    pub config: InstallationConfig,
    pub auth_security: Arc<AuthSecurity>,
    pub ingest_writer: IngestWriter,
}

impl InstalledState {
    pub fn new(
        database: DatabaseConnection,
        config: InstallationConfig,
        auth_security: Arc<AuthSecurity>,
    ) -> Self {
        let ingest_writer = IngestWriter::new(database.clone());
        Self {
            database,
            config,
            auth_security,
            ingest_writer,
        }
    }
}

pub struct AppState {
    pub runtime: RuntimeConfig,
    installed: RwLock<Option<Arc<InstalledState>>>,
    pub setup_lock: Mutex<()>,
    pub live_updates: broadcast::Sender<String>,
    ingest_gate: Arc<Semaphore>,
    analytics_gate: Arc<Semaphore>,
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
            let auth_security = Arc::new(AuthSecurity::new(runtime.password_pepper.as_bytes())?);
            Some(Arc::new(InstalledState::new(
                database,
                config,
                auth_security,
            )))
        } else if let Some(url) = &runtime.database_url_override {
            match database::connect(url).await {
                Ok(database) => {
                    database::migrate(&database).await?;
                    if is_database_installed(&database).await {
                        let config = InstallationConfig {
                            database_url: url.clone(),
                            locale: "en".into(),
                            timezone: "UTC".into(),
                            secure_cookie: false,
                        };
                        config
                            .write_atomic(&runtime.config_path)
                            .map_err(|_| AppError::Internal)?;
                        let auth_security =
                            Arc::new(AuthSecurity::new(runtime.password_pepper.as_bytes())?);
                        Some(Arc::new(InstalledState::new(
                            database,
                            config,
                            auth_security,
                        )))
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
                        database::migrate(&database).await?;
                        if is_database_installed(&database).await {
                            let config = InstallationConfig {
                                database_url: db_url,
                                locale: "en".into(),
                                timezone: "UTC".into(),
                                secure_cookie: false,
                            };
                            config
                                .write_atomic(&runtime.config_path)
                                .map_err(|_| AppError::Internal)?;
                            let auth_security =
                                Arc::new(AuthSecurity::new(runtime.password_pepper.as_bytes())?);
                            Some(Arc::new(InstalledState::new(
                                database,
                                config,
                                auth_security,
                            )))
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

        if let Some(installed) = installed.as_deref() {
            spawn_background_workers(installed);
        }

        let (live_updates, _) = broadcast::channel(256);
        Ok(Self {
            runtime,
            installed: RwLock::new(installed),
            setup_lock: Mutex::new(()),
            live_updates,
            ingest_gate: Arc::new(Semaphore::new(MAX_IN_FLIGHT_INGEST_REQUESTS)),
            analytics_gate: Arc::new(Semaphore::new(MAX_IN_FLIGHT_ANALYTICS_QUERIES)),
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

    pub fn try_acquire_ingest(&self) -> Result<OwnedSemaphorePermit, AppError> {
        self.ingest_gate
            .clone()
            .try_acquire_owned()
            .map_err(|_| AppError::TooManyRequests)
    }

    pub fn try_acquire_analytics(&self) -> Result<OwnedSemaphorePermit, AppError> {
        self.analytics_gate
            .clone()
            .try_acquire_owned()
            .map_err(|_| AppError::TooManyRequests)
    }

    pub async fn update_installed_config<F>(
        &self,
        update_fn: F,
    ) -> Result<InstallationConfig, AppError>
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
                ingest_writer: installed_ref.ingest_writer.clone(),
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
        spawn_background_workers(&state);
        *guard = Some(Arc::new(state));
        Ok(())
    }
}

fn spawn_background_workers(state: &InstalledState) {
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
            .map(|v| v == "true")
            .unwrap_or(false),
        _ => false,
    }
}
