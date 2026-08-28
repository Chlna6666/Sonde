use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr,
    sea_query::{Alias, Expr, ExprTrait, Query},
};
use tracing::{info, warn};

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

        // Delete from events
        let del_events = Query::delete()
            .from_table(Alias::new("events"))
            .and_where(Expr::col(Alias::new("application_id")).eq(&app_id))
            .and_where(Expr::col(Alias::new("timestamp")).lt(cutoff))
            .to_owned();
        if let Ok(res) = database.execute(&del_events).await {
            report.events_deleted += res.rows_affected();
        }

        // Delete from metric_points
        let del_metrics = Query::delete()
            .from_table(Alias::new("metric_points"))
            .and_where(Expr::col(Alias::new("application_id")).eq(&app_id))
            .and_where(Expr::col(Alias::new("timestamp")).lt(cutoff))
            .to_owned();
        if let Ok(res) = database.execute(&del_metrics).await {
            report.metrics_deleted += res.rows_affected();
        }

        // Delete from logs
        let del_logs = Query::delete()
            .from_table(Alias::new("logs"))
            .and_where(Expr::col(Alias::new("application_id")).eq(&app_id))
            .and_where(Expr::col(Alias::new("timestamp")).lt(cutoff))
            .to_owned();
        if let Ok(res) = database.execute(&del_logs).await {
            report.logs_deleted += res.rows_affected();
        }

        // Delete from daily_aggregates
        let del_aggregates = Query::delete()
            .from_table(Alias::new("daily_aggregates"))
            .and_where(Expr::col(Alias::new("application_id")).eq(&app_id))
            .and_where(Expr::col(Alias::new("updated_at")).lt(cutoff))
            .to_owned();
        let _ = database.execute(&del_aggregates).await;

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
