pub mod alert_delivery;
pub mod alerts;
pub mod applications;
pub mod application_delete;
pub mod auth;
pub mod auth_state;
pub mod backup_archive;
pub mod backup_models;
pub mod backup_restore;
pub mod backup_validation;
pub mod application_backup;
pub mod device_query;
pub mod device_risk;
pub mod device_state;
pub mod dimension_restore;
pub mod dimension_rollup;
pub mod error_query;
pub mod errors;
pub mod explorer;
pub mod first_seen;
pub mod imports;
pub mod ingest_auth;
pub mod ingest_bootstrap;
pub mod ingest_nonce;
pub mod job_lease;
pub mod log_error_rollup;
mod migration;
pub mod query;
pub mod rollups;
pub mod stats;
pub mod telemetry_count;
pub mod telemetry;
pub mod trends;
pub mod user_rollup;
pub mod version_dimension;

// Transitional aliases for the atomic rename. All call sites are migrated in the following commit
// and these aliases are then removed; they are not part of Sonde's supported API surface.
#[doc(hidden)] pub use alert_delivery as alert_delivery_repo;
#[doc(hidden)] pub use alerts as alert_repo;
#[doc(hidden)] pub use applications as app_repo;
#[doc(hidden)] pub use application_delete as application_delete_repo;
#[doc(hidden)] pub use auth as auth_repo;
#[doc(hidden)] pub use auth_state as auth_state_repo;
#[doc(hidden)] pub use backup_archive as backup_archive_repo;
#[doc(hidden)] pub use backup_models as backup_repo;
#[doc(hidden)] pub use backup_restore as backup_archive_restore_repo;
#[doc(hidden)] pub use backup_validation as backup_archive_validation_repo;
#[doc(hidden)] pub use application_backup as legacy_backup_repo;
#[doc(hidden)] pub use device_query as device_query_repo;
#[doc(hidden)] pub use device_risk as device_risk_repo;
#[doc(hidden)] pub use device_state as device_state_repo;
#[doc(hidden)] pub use dimension_restore as dimension_restore_repo;
#[doc(hidden)] pub use dimension_rollup as dimension_rollup_repo;
#[doc(hidden)] pub use error_query as error_query_repo;
#[doc(hidden)] pub use errors as error_repo;
#[doc(hidden)] pub use explorer as explorer_repo;
#[doc(hidden)] pub use first_seen as first_seen_repo;
#[doc(hidden)] pub use imports as import_repo;
#[doc(hidden)] pub use ingest_auth as ingest_auth_repo;
#[doc(hidden)] pub use ingest_bootstrap as ingest_bootstrap_repo;
#[doc(hidden)] pub use ingest_nonce as ingest_nonce_repo;
#[doc(hidden)] pub use job_lease as job_lease_repo;
#[doc(hidden)] pub use log_error_rollup as log_error_rollup_repo;
#[doc(hidden)] pub use rollups as rollup_repo;
#[doc(hidden)] pub use stats as stats_repo;
#[doc(hidden)] pub use telemetry_count as telemetry_count_repo;
#[doc(hidden)] pub use telemetry as telemetry_repo;
#[doc(hidden)] pub use trends as trend_repo;
#[doc(hidden)] pub use user_rollup as user_rollup_repo;
#[doc(hidden)] pub use version_dimension as version_dimension_repo;

use sea_orm::{ConnectOptions, ConnectionTrait, Database, DatabaseConnection, DbErr};
use sea_orm_migration::MigratorTrait;
use std::time::Duration;

pub use migration::Migrator;

pub async fn connect(database_url: &str) -> Result<DatabaseConnection, DbErr> {
    let is_sqlite = database_url.starts_with("sqlite:");
    let mut options = ConnectOptions::new(database_url);
    options
        .max_connections(if is_sqlite { 8 } else { 50 })
        .min_connections(if is_sqlite { 1 } else { 2 })
        .connect_timeout(Duration::from_secs(10))
        .acquire_timeout(Duration::from_secs(10))
        .idle_timeout(Duration::from_secs(300))
        .max_lifetime(Duration::from_secs(1800))
        .sqlx_logging(false);

    if is_sqlite {
        options.after_connect(|connection| {
            Box::pin(async move {
                connection
                    .execute_unprepared(
                        "PRAGMA busy_timeout=5000; PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA wal_autocheckpoint=1000;",
                    )
                    .await?;
                Ok(())
            })
        });
    }

    Database::connect(options).await
}

pub async fn migrate(database: &DatabaseConnection) -> Result<(), DbErr> {
    Migrator::up(database, None).await
}
