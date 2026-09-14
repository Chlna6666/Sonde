pub mod activity_stats;
pub mod alert_delivery;
pub mod alerts;
pub mod application_backup;
pub mod application_delete;
mod application_transfer;
pub mod applications;
pub mod auth;
pub mod auth_state;
pub mod backup_archive;
mod backup_records;
pub mod backup_restore;
pub mod backup_validation;
pub mod device_activity;
pub mod device_activity_backfill;
pub mod device_history;
pub mod device_identity;
pub mod device_query;
pub mod device_risk;
pub mod device_session;
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
pub mod telemetry;
pub mod telemetry_count;
pub mod trends;
pub mod user_rollup;
pub mod version_dimension;

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
    Migrator::up(database, None).await?;
    auth::ensure_builtin_roles(database).await
}
