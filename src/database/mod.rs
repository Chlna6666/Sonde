pub mod alert_repo;
pub mod app_repo;
pub mod auth_repo;
pub mod auth_state_repo;
pub mod backup_repo;
pub mod error_query_repo;
pub mod error_repo;
pub mod explorer_repo;
pub mod import_repo;
pub mod ingest_auth_repo;
pub mod job_lease_repo;
mod migration;
pub mod query;
pub mod rollup_repo;
pub mod stats_repo;
pub mod telemetry_repo;

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
