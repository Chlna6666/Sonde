use std::sync::Arc;

use sea_orm::DatabaseConnection;
use tokio::sync::{
    Mutex, OwnedSemaphorePermit, RwLock, Semaphore, TryAcquireError, broadcast,
};

use crate::{
    config::{InstallationConfig, RuntimeConfig},
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
        let installed = crate::bootstrap::load_installed(&runtime).await?;
        if let Some(installed) = installed.as_deref() {
            crate::bootstrap::spawn_background_workers(installed);
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
            .map_err(|error| admission_error("ingest admission gate", error))
    }

    pub fn try_acquire_analytics(&self) -> Result<OwnedSemaphorePermit, AppError> {
        self.analytics_gate
            .clone()
            .try_acquire_owned()
            .map_err(|error| admission_error("analytics admission gate", error))
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
                .map_err(|error| AppError::internal("write installation config", error))?;
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
        crate::bootstrap::spawn_background_workers(&state);
        *guard = Some(Arc::new(state));
        Ok(())
    }
}

fn admission_error(context: &'static str, error: TryAcquireError) -> AppError {
    match error {
        TryAcquireError::NoPermits => AppError::TooManyRequests,
        TryAcquireError::Closed => AppError::unavailable(context, error),
    }
}
