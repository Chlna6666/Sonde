use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr,
    sea_query::{Alias, Expr, ExprTrait, Query},
};
use tracing::{info, warn};

const DELETE_BATCH_SIZE: u64 = 5_000;

#[derive(Debug, Default)]
pub struct RetentionReport {
    pub apps_processed: usize,
    pub events_deleted: u64,
    pub metrics_deleted: u64,
    pub logs_deleted: u64,
}

pub async fn run_retention_sweep(database: &DatabaseConnection) -> Result<RetentionReport, DbErr> {
    let select_apps = Query::select()
        .columns(["id", "name", "retention_days"].map(Alias::new))
        .from(Alias::new("applications"))
        .and_where(Expr::col(Alias::new("retention_days")).gt(0))
        .to_owned();

    let app_rows = database.query_all(&select_apps).await?;
    let mut report = RetentionReport::default();
    let now = chrono::Utc::now().timestamp_millis();

    for row in app_rows {
        let app_id: String = row.try_get("", "id")?;
        let app_name: String = row.try_get("", "name")?;
        let retention_days: i32 = row.try_get("", "retention_days")?;

        if retention_days <= 0 {
            continue;
        }

        let cutoff = now - (retention_days as i64) * 86_400_000;
        report.apps_processed += 1;

        report.events_deleted +=
            delete_in_batches(database, "events", "timestamp", &app_id, cutoff).await?;
        report.metrics_deleted +=
            delete_in_batches(database, "metric_points", "timestamp", &app_id, cutoff).await?;
        report.logs_deleted +=
            delete_in_batches(database, "logs", "timestamp", &app_id, cutoff).await?;
        let _ = delete_in_batches(
            database,
            "daily_aggregates",
            "updated_at",
            &app_id,
            cutoff,
        )
        .await?;

        info!(
            app_id = %app_id,
            app_name = %app_name,
            retention_days = %retention_days,
            "retention sweep completed for application"
        );
    }

    if report.apps_processed > 0 {
        info!(
            apps = report.apps_processed,
            events_pruned = report.events_deleted,
            metrics_pruned = report.metrics_deleted,
            logs_pruned = report.logs_deleted,
            "periodic data retention sweep summary"
        );
    }

    Ok(report)
}

async fn delete_in_batches(
    database: &DatabaseConnection,
    table: &str,
    timestamp_column: &str,
    application_id: &str,
    cutoff: i64,
) -> Result<u64, DbErr> {
    let mut deleted = 0_u64;

    loop {
        let select = Query::select()
            .column(Alias::new("id"))
            .from(Alias::new(table))
            .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
            .and_where(Expr::col(Alias::new(timestamp_column)).lt(cutoff))
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
