use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr,
    sea_query::{Alias, Query},
};

use super::dimension_rollup_repo;

/// Dimension rollups are derived cache state, not authoritative backup data.
///
/// Invalidate the readiness marker before deleting cached rows so concurrent statistics requests
/// immediately fall back to raw telemetry. Historical event days are then marked dirty again and
/// the normal rollup worker rebuilds the cache generation-safely.
pub async fn reset_after_full_restore(database: &DatabaseConnection) -> Result<usize, DbErr> {
    dimension_rollup_repo::invalidate_dimension_backfill(database).await?;

    let clear_dimensions = Query::delete()
        .from_table(Alias::new("telemetry_daily_dimensions"))
        .to_owned();
    database.execute(&clear_dimensions).await?;

    dimension_rollup_repo::seed_historical_dimension_dirty_days_once(database).await
}
