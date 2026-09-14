use std::sync::Arc;

use sea_orm::DatabaseConnection;
use tokio::sync::{Mutex, OwnedSemaphorePermit, RwLock, Semaphore, TryAcquireError, broadcast};

use crate::{
    config::{InstallationConfig, MasterKey, RuntimeConfig},
    error::AppError,
    secret_cipher::SecretCipher,
    security::{AuthSecurity, PreInstallThrottle},
    services::ingest_writer::IngestWriter,
};
use tracing::warn;

const MAX_IN_FLIGHT_INGEST_REQUESTS: usize = 64;
const MAX_IN_FLIGHT_ANALYTICS_QUERIES: usize = 8;
const SETUP_REQUESTS_PER_MINUTE: u64 = 30;
const MAX_SETUP_THROTTLE_ENTRIES: usize = 4_096;

/// A telemetry activity notification fanned out to live-update subscribers.
///
/// The application id travels alongside the serialized payload so every subscriber can drop
/// updates for applications it is not allowed to read.
#[derive(Clone, Debug)]
pub struct LiveUpdate {
    pub application_id: String,
    pub payload: String,
}

pub struct InstalledState {
    pub database: DatabaseConnection,
    pub config: InstallationConfig,
    pub auth_security: Arc<AuthSecurity>,
    /// AEAD cipher for secrets that must stay recoverable (TOTP shared secrets).
    pub secret_cipher: SecretCipher,
    pub ingest_writer: IngestWriter,
}

impl InstalledState {
    pub fn new(
        database: DatabaseConnection,
        config: InstallationConfig,
        auth_security: Arc<AuthSecurity>,
        master_key: &MasterKey,
    ) -> Result<Self, AppError> {
        let secret_cipher = SecretCipher::new(master_key.as_bytes())?;
        let ingest_writer = IngestWriter::new(database.clone());
        Ok(Self {
            database,
            config,
            auth_security,
            secret_cipher,
            ingest_writer,
        })
    }
}

pub struct AppState {
    pub runtime: RuntimeConfig,
    installed: RwLock<Option<Arc<InstalledState>>>,
    pub setup_lock: Mutex<()>,
    setup_token: RwLock<Option<String>>,
    pub live_updates: broadcast::Sender<LiveUpdate>,
    ingest_gate: Arc<Semaphore>,
    analytics_gate: Arc<Semaphore>,
    setup_throttle: PreInstallThrottle,
}

impl AppState {
    pub async fn load(runtime: RuntimeConfig) -> Result<Self, AppError> {
        let installed = crate::bootstrap::load_installed(&runtime).await?;
        if let Some(installed) = installed.as_deref() {
            crate::bootstrap::spawn_background_workers(installed);
        }

        let (setup_token, setup_token_generated) = match runtime.setup_token.clone() {
            Some(token) => (Some(token), false),
            None => (Some(crate::auth::random_token(32)), true),
        };
        if setup_token_generated {
            warn!(
                "no SONDE_SETUP_TOKEN configured; a one-time setup token was generated and is \
                 required by /api/v1/setup/*: {}",
                setup_token.as_deref().unwrap_or_default()
            );
        }

        let (live_updates, _) = broadcast::channel(256);
        Ok(Self {
            runtime,
            installed: RwLock::new(installed),
            setup_lock: Mutex::new(()),
            setup_token: RwLock::new(setup_token),
            live_updates,
            ingest_gate: Arc::new(Semaphore::new(MAX_IN_FLIGHT_INGEST_REQUESTS)),
            analytics_gate: Arc::new(Semaphore::new(MAX_IN_FLIGHT_ANALYTICS_QUERIES)),
            setup_throttle: PreInstallThrottle::new(SETUP_REQUESTS_PER_MINUTE, 60_000),
        })
    }

    /// Verifies the one-time setup token presented on a pre-installation endpoint.
    ///
    /// `/api/v1/setup/*` runs before any account exists, so this token is the only identity
    /// available; without it an attacker who reaches the port first could complete the setup
    /// wizard and take over the instance.
    pub async fn verify_setup_token(&self, presented: Option<&str>) -> Result<(), AppError> {
        let Some(expected) = self.setup_token.read().await.clone() else {
            return Err(AppError::Forbidden);
        };
        let Some(presented) = presented else {
            return Err(AppError::Unauthorized);
        };
        if crate::auth::constant_time_eq(presented, &expected) {
            Ok(())
        } else {
            Err(AppError::Unauthorized)
        }
    }

    /// Retires the setup token once the wizard completed, so it cannot be replayed.
    pub async fn consume_setup_token(&self) {
        *self.setup_token.write().await = None;
    }

    /// Throttles the unauthenticated setup wizard endpoints, which are reachable before any
    /// installation (and therefore before the login gate) exists.
    pub async fn charge_setup_request(&self, client_ip: &str) -> bool {
        self.setup_throttle
            .charge(client_ip, MAX_SETUP_THROTTLE_ENTRIES)
            .await
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
                secret_cipher: installed_ref.secret_cipher.clone(),
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
