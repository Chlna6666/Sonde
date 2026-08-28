use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr, TransactionTrait,
    sea_query::{Alias, Expr, ExprTrait, Query},
};

const DELETE_ID_CHUNK: usize = 500;
const FIRST_SEEN_BACKFILL_KEY: &str = "telemetry_first_seen_backfill_v1";

pub async fn delete_application_exact(
    database: &DatabaseConnection,
    application_id: &str,
) -> Result<(), DbErr> {
    let transaction = database.begin().await?;

    // Delivery rows do not carry application_id directly, so remove them through their rule ids
    // before deleting the rules themselves.
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

    // Delete children before environments/application. These tables intentionally do not all carry
    // foreign keys because Sonde supports three database engines and derived cache rows use virtual
    // scopes, so lifecycle cleanup must be explicit.
    for table in [
        "error_occurrences",
        "error_groups",
        "telemetry_daily_rollups",
        "telemetry_daily_dimensions",
        "telemetry_daily_user_sets",
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

    // The global first-seen scope deduplicates the same anonymous user across applications. Once one
    // application disappears, a global row may need to move to that user's next surviving event.
    // Rebuild the compact derived index from authoritative remaining events rather than trying to
    // infer provenance from its hashed row id.
    for table in [
        "telemetry_first_seen_backfill_days",
        "telemetry_user_first_seen",
    ] {
        let delete = Query::delete().from_table(Alias::new(table)).to_owned();
        transaction.execute(&delete).await?;
    }
    let clear_first_seen_state = Query::delete()
        .from_table(Alias::new("system_state"))
        .and_where(Expr::col(Alias::new("key")).eq(FIRST_SEEN_BACKFILL_KEY))
        .to_owned();
    transaction.execute(&clear_first_seen_state).await?;

    let delete_app = Query::delete()
        .from_table(Alias::new("applications"))
        .and_where(Expr::col(Alias::new("id")).eq(application_id))
        .to_owned();
    transaction.execute(&delete_app).await?;
    transaction.commit().await
}
