use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr,
    sea_query::{Alias, Query},
};

use super::{
    device_activity_backfill, dimension_rollup, first_seen, log_error_rollup, rollups, user_rollup,
};

/// Telemetry rollups, activity indexes and first-seen indexes are derived state, not authoritative
/// backup data.
///
/// Invalidate readiness before deleting cached rows so concurrent statistics requests immediately
/// stop treating a prior rebuild as complete. Historical source days are then marked dirty again and
/// background workers rebuild every derived structure from the restored authoritative rows.
pub async fn reset_after_full_restore(database: &DatabaseConnection) -> Result<usize, DbErr> {
    rollups::invalidate_rollup_backfill(database).await?;
    dimension_rollup::invalidate_dimension_backfill(database).await?;
    user_rollup::invalidate_user_backfill(database).await?;
    log_error_rollup::invalidate_backfill(database).await?;
    first_seen::invalidate(database).await?;
    device_activity_backfill::invalidate(database).await?;

    // Derived caches are deliberately rebuilt. A backup snapshot may contain raw rows written after
    // the latest worker fold, while dirty markers themselves are ephemeral and not archived.
    for table in [
        "telemetry_device_activity_hours",
        "telemetry_device_activity_days",
        "telemetry_daily_rollups",
        "telemetry_daily_dimensions",
        "telemetry_daily_user_sets",
        "telemetry_daily_log_errors",
    ] {
        let clear = Query::delete().from_table(Alias::new(table)).to_owned();
        database.execute(&clear).await?;
    }

    let base_days = rollups::seed_historical_dirty_days_once(database).await?;
    let dimension_days = dimension_rollup::seed_historical_dimension_dirty_days_once(database).await?;
    let user_days = user_rollup::seed_historical_user_dirty_days_once(database).await?;
    let log_error_days = log_error_rollup::seed_historical_dirty_days_once(database).await?;
    Ok([base_days, dimension_days, user_days, log_error_days]
        .into_iter()
        .max()
        .unwrap_or(0))
}
