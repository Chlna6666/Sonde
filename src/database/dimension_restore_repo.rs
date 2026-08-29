use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr,
    sea_query::{Alias, Query},
};

use super::{
    dimension_rollup_repo, first_seen_repo, log_error_rollup_repo, rollup_repo, user_rollup_repo,
};

/// Telemetry rollups and first-seen indexes are derived state, not authoritative backup data.
///
/// Invalidate readiness before deleting cached rows so concurrent statistics requests immediately
/// fall back to raw telemetry. Historical source days are then marked dirty again and background
/// workers rebuild every derived structure from the restored authoritative rows.
pub async fn reset_after_full_restore(database: &DatabaseConnection) -> Result<usize, DbErr> {
    dimension_rollup_repo::invalidate_dimension_backfill(database).await?;
    user_rollup_repo::invalidate_user_backfill(database).await?;
    log_error_rollup_repo::invalidate_backfill(database).await?;
    first_seen_repo::invalidate(database).await?;

    for table in [
        "telemetry_daily_dimensions",
        "telemetry_daily_user_sets",
        "telemetry_daily_log_errors",
    ] {
        let clear = Query::delete().from_table(Alias::new(table)).to_owned();
        database.execute(&clear).await?;
    }

    let dimension_days =
        dimension_rollup_repo::seed_historical_dimension_dirty_days_once(database).await?;
    let user_days = user_rollup_repo::seed_historical_user_dirty_days_once(database).await?;
    if dimension_days > 0 || user_days > 0 {
        // Backfill helpers predate source masks. If their insert collides with a metric/log-only
        // marker, explicitly promote pending work to EVENT so event-derived caches cannot be skipped.
        database
            .execute_unprepared(&format!(
                "UPDATE telemetry_dirty_days SET source_mask = source_mask | {}",
                rollup_repo::DIRTY_SOURCE_EVENT
            ))
            .await?;
    }
    let log_error_days = log_error_rollup_repo::seed_historical_dirty_days_once(database).await?;
    Ok(dimension_days.max(user_days).max(log_error_days))
}
