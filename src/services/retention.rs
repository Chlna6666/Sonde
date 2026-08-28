use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr,
    sea_query::{Alias, Expr, ExprTrait, Query},
};
use tracing::{info, warn};

use crate::database::{first_seen_repo, rollup_repo, telemetry_repo::TelemetryScope};

/// Each deleted id becomes one bind variable in the follow-up `IN (...)` statement. Keep this
/// comfortably below SQLite's historical 999-variable limit and leave room for driver-added binds.
const DELETE_BATCH_SIZE: u64 = 500;

#[derive(Debug, Default)]
pub struct RetentionReport {
    pub apps_processed: usize,
    pub events_deleted: u64,
    pub metrics_deleted: u64,
    pub logs_deleted: u64,
    pub error_occurrences_deleted: u64,
    pub error_groups_deleted: u64,
    pub rollups_deleted: u64,
    pub dimension_rollups_deleted: u64,
    pub user_rollups_deleted: u64,
    pub dirty_days_deleted: u64,
}

pub async fn run_retention_sweep(database: &DatabaseConnection) -> Result<RetentionReport, DbErr> {
    let select_apps = Query::select()
        .columns(["id", "name", "retention_days"].map(Alias::new))
        .from(Alias::new("applications"))
        .and_where(Expr::col(Alias::new("retention_days")).gt(0))
        .to_owned();

    let app_rows = database.query_all(&select_apps).await?;
    let mut report = RetentionReport::default();
    let mut first_seen_needs_rebuild = false;
    let now = chrono::Utc::now().timestamp_millis();

    for row in app_rows {
        let app_id: String = row.try_get("", "id")?;
        let app_name: String = row.try_get("", "name")?;
        let retention_days: i32 = row.try_get("", "retention_days")?;

        if retention_days <= 0 {
            continue;
        }

        let cutoff = now - (retention_days as i64) * 86_400_000;
        let cutoff_day = chrono::DateTime::from_timestamp_millis(cutoff)
            .map(|value| value.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "1970-01-01".into());
        report.apps_processed += 1;

        let events_deleted =
            delete_in_batches(database, "events", "timestamp", &app_id, cutoff).await?;
        let metrics_deleted =
            delete_in_batches(database, "metric_points", "timestamp", &app_id, cutoff).await?;
        let logs_deleted =
            delete_in_batches(database, "logs", "timestamp", &app_id, cutoff).await?;
        let error_occurrences_deleted = delete_in_batches(
            database,
            "error_occurrences",
            "timestamp",
            &app_id,
            cutoff,
        )
        .await?;
        first_seen_needs_rebuild |= events_deleted > 0;
        report.events_deleted = report.events_deleted.saturating_add(events_deleted);
        report.metrics_deleted = report.metrics_deleted.saturating_add(metrics_deleted);
        report.logs_deleted = report.logs_deleted.saturating_add(logs_deleted);
        report.error_occurrences_deleted = report
            .error_occurrences_deleted
            .saturating_add(error_occurrences_deleted);
        report.error_groups_deleted +=
            delete_in_batches(database, "error_groups", "last_seen", &app_id, cutoff).await?;
        report.rollups_deleted += delete_string_in_batches(
            database,
            "telemetry_daily_rollups",
            "day",
            &app_id,
            &cutoff_day,
        )
        .await?;
        report.dimension_rollups_deleted += delete_string_in_batches(
            database,
            "telemetry_daily_dimensions",
            "day",
            &app_id,
            &cutoff_day,
        )
        .await?;
        report.user_rollups_deleted += delete_string_in_batches(
            database,
            "telemetry_daily_user_sets",
            "day",
            &app_id,
            &cutoff_day,
        )
        .await?;
        report.dirty_days_deleted += delete_string_in_batches(
            database,
            "telemetry_dirty_days",
            "day",
            &app_id,
            &cutoff_day,
        )
        .await?;
        let _ = delete_in_batches(
            database,
            "daily_aggregates",
            "updated_at",
            &app_id,
            cutoff,
        )
        .await?;

        // Raw retention is millisecond-precise while rollups are day-scoped. Recompute only the
        // data sources whose rows were actually removed from the cutoff day; this avoids forcing
        // event/user/dimension scans after a metric-only or log-only retention change.
        let mut source_mask = 0_i64;
        if events_deleted > 0 {
            source_mask |= rollup_repo::DIRTY_SOURCE_EVENT;
        }
        if metrics_deleted > 0 {
            source_mask |= rollup_repo::DIRTY_SOURCE_METRIC;
        }
        if logs_deleted > 0 {
            source_mask |= rollup_repo::DIRTY_SOURCE_LOG;
        }
        if error_occurrences_deleted > 0 {
            source_mask |= rollup_repo::DIRTY_SOURCE_ERROR;
        }
        if source_mask != 0 {
            mark_retention_boundary_dirty(database, &app_id, cutoff, source_mask).await?;
        }

        info!(
            app_id = %app_id,
            app_name = %app_name,
            retention_days = %retention_days,
            "retention sweep completed for application"
        );
    }

    // First-seen is a derived index over retained events. Deleting an old event can move a user's
    // first retained occurrence forward, so a monotonic MIN-only incremental update is insufficient.
    // Rebuild asynchronously from the remaining authoritative rows instead of preserving deleted
    // user history past the configured retention boundary.
    if first_seen_needs_rebuild {
        first_seen_repo::invalidate(database).await?;
    }

    crate::database::auth_state_repo::cleanup_expired(database).await?;

    if report.apps_processed > 0 {
        info!(
            apps = report.apps_processed,
            events_pruned = report.events_deleted,
            metrics_pruned = report.metrics_deleted,
            logs_pruned = report.logs_deleted,
            error_occurrences_pruned = report.error_occurrences_deleted,
            error_groups_pruned = report.error_groups_deleted,
            rollups_pruned = report.rollups_deleted,
            dimension_rollups_pruned = report.dimension_rollups_deleted,
            user_rollups_pruned = report.user_rollups_deleted,
            dirty_days_pruned = report.dirty_days_deleted,
            "periodic data retention sweep summary"
        );
    }

    Ok(report)
}

async fn mark_retention_boundary_dirty(
    database: &DatabaseConnection,
    application_id: &str,
    cutoff: i64,
    source_mask: i64,
) -> Result<(), DbErr> {
    let query = Query::select()
        .column(Alias::new("id"))
        .from(Alias::new("environments"))
        .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
        .to_owned();
    for row in database.query_all(&query).await? {
        let scope = TelemetryScope {
            application_id: application_id.to_owned(),
            environment_id: row.try_get("", "id")?,
        };
        rollup_repo::mark_dirty_timestamps_for_source(database, &scope, source_mask, [cutoff])
            .await?;
    }
    Ok(())
}

async fn delete_in_batches(
    database: &DatabaseConnection,
    table: &str,
    timestamp_column: &str,
    application_id: &str,
    cutoff: i64,
) -> Result<u64, DbErr> {
    delete_matching_ids(
        database,
        table,
        Expr::col(Alias::new(timestamp_column)).lt(cutoff),
        application_id,
    )
    .await
}

async fn delete_string_in_batches(
    database: &DatabaseConnection,
    table: &str,
    column: &str,
    application_id: &str,
    cutoff: &str,
) -> Result<u64, DbErr> {
    delete_matching_ids(
        database,
        table,
        Expr::col(Alias::new(column)).lt(cutoff),
        application_id,
    )
    .await
}

async fn delete_matching_ids(
    database: &DatabaseConnection,
    table: &str,
    cutoff_condition: sea_orm::sea_query::SimpleExpr,
    application_id: &str,
) -> Result<u64, DbErr> {
    let mut deleted = 0_u64;

    loop {
        let select = Query::select()
            .column(Alias::new("id"))
            .from(Alias::new(table))
            .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
            .and_where(cutoff_condition.clone())
            .limit(DELETE_BATCH_SIZE)
            .to_owned();
        let rows = database.query_all(&select).await?;
        if rows.is_empty() {
            break;
        }

        let ids = rows
            .into_iter()
            .map(|row| row.try_get::<String>("", "id"))
            .collect::<Result<Vec<_>, _>>()?;
        let batch_len = ids.len();
        let delete = Query::delete()
            .from_table(Alias::new(table))
            .and_where(Expr::col(Alias::new("id")).is_in(ids))
            .to_owned();
        let affected = database.execute(&delete).await?.rows_affected();
        deleted = deleted.saturating_add(affected);

        if affected == 0 || batch_len < DELETE_BATCH_SIZE as usize {
            break;
        }
        tokio::task::yield_now().await;
    }

    Ok(deleted)
}

pub fn spawn_retention_worker(database: DatabaseConnection) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(4 * 3600));
        loop {
            interval.tick().await;
            if let Err(err) = run_retention_sweep(&database).await {
                warn!(error = %err, "retention sweep worker encountered an error");
            }
        }
    });
}
