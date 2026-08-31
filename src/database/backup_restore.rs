use std::path::Path;

use sea_orm::{
    ConnectionTrait, DatabaseConnection,
    sea_query::{Alias, Expr, ExprTrait, Query},
};

use super::backup_archive::BackupError;

mod engine;

const LEGACY_LOG_ERROR_ROLLUP_BACKFILL_KEY: &str = "telemetry_log_error_rollup_backfill_v2";

pub async fn restore_full_system_exact(
    database: &DatabaseConnection,
    path: &Path,
) -> Result<u64, BackupError> {
    let restored = engine::restore_full_system_exact(database, path).await?;
    clear_legacy_derived_state(database).await?;
    Ok(restored)
}

async fn clear_legacy_derived_state(database: &DatabaseConnection) -> Result<(), BackupError> {
    let delete = Query::delete()
        .from_table(Alias::new("system_state"))
        .and_where(
            Expr::col(Alias::new("key")).eq(LEGACY_LOG_ERROR_ROLLUP_BACKFILL_KEY),
        )
        .to_owned();
    database.execute(&delete).await?;
    Ok(())
}
