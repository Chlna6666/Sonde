use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr, TransactionTrait,
    sea_query::{Alias, Expr, ExprTrait, Query},
};

const DELETE_ID_CHUNK: usize = 500;
const FIRST_SEEN_BACKFILL_KEY: &str = "telemetry_first_seen_backfill";
const FIRST_SEEN_CURSOR_KEY: &str = "telemetry_first_seen_backfill_cursor";

pub async fn delete_application_exact(
    database: &DatabaseConnection,
    application_id: &str,
) -> Result<(), DbErr> {
    let transaction = database.begin().await?;

    let rule_query = Query::select()
        .column(Alias::new("id"))
        .from(Alias::new("alert_rules"))
        .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
        .to_owned();
    let rule_ids = transaction
        .query_all(&rule_query)
        .await?
        .into_iter()
        .map(|row| row.try_get::<String>("", "id"))
        .collect::<Result<Vec<_>, _>>()?;
    for chunk in rule_ids.chunks(DELETE_ID_CHUNK) {
        let delete = Query::delete()
            .from_table(Alias::new("alert_deliveries"))
            .and_where(Expr::col(Alias::new("rule_id")).is_in(chunk.iter().cloned()))
            .to_owned();
        transaction.execute(&delete).await?;
    }

    for table in [
        "error_occurrences",
        "error_groups",
        "telemetry_device_sessions",
        "telemetry_device_activity_hours",
        "telemetry_device_activity_days",
        "telemetry_devices",
        "telemetry_daily_rollups",
        "telemetry_daily_dimensions",
        "telemetry_daily_user_sets",
        "telemetry_daily_log_errors",
        "telemetry_dirty_days",
        "telemetry_first_seen_backfill_days",
        "daily_aggregates",
        "events",
        "metric_points",
        "logs",
        "alert_rules",
        "import_runs",
        "api_keys",
        "environments",
        "role_bindings",
    ] {
        let delete = Query::delete()
            .from_table(Alias::new(table))
            .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
            .to_owned();
        transaction.execute(&delete).await?;
    }

    for key in [FIRST_SEEN_BACKFILL_KEY, FIRST_SEEN_CURSOR_KEY] {
        let clear = Query::delete()
            .from_table(Alias::new("system_state"))
            .and_where(Expr::col(Alias::new("key")).eq(key))
            .to_owned();
        transaction.execute(&clear).await?;
    }

    let delete_app = Query::delete()
        .from_table(Alias::new("applications"))
        .and_where(Expr::col(Alias::new("id")).eq(application_id))
        .to_owned();
    transaction.execute(&delete_app).await?;
    transaction.commit().await
}
