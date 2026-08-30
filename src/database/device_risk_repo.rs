use sea_orm::{ConnectionTrait, DatabaseConnection, DbErr, sea_query::{Alias, Expr, ExprTrait, Query}};

pub async fn risk_score_for_device(
    database: &DatabaseConnection,
    application_id: &str,
    environment_id: &str,
    device_hash: &str,
) -> Result<Option<i32>, DbErr> {
    let query = Query::select()
        .column(Alias::new("risk_score"))
        .from(Alias::new("telemetry_devices"))
        .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
        .and_where(Expr::col(Alias::new("environment_id")).eq(environment_id))
        .and_where(Expr::col(Alias::new("device_hash")).eq(device_hash))
        .limit(1)
        .to_owned();
    database
        .query_one(&query)
        .await?
        .map(|row| row.try_get::<i32>("", "risk_score"))
        .transpose()
}

/// Decay device risk after a sustained period without a new anomaly.
///
/// This is deliberately gradual: one point per leased maintenance cycle. It never blocks ingest
/// and keeps historic anomaly context while allowing normal devices to recover over time.
pub async fn decay_scores(
    database: &impl ConnectionTrait,
    anomaly_cutoff: i64,
) -> Result<u64, DbErr> {
    let query = Query::update()
        .table(Alias::new("telemetry_devices"))
        .value(
            Alias::new("risk_score"),
            Expr::cust("CASE WHEN risk_score > 0 THEN risk_score - 1 ELSE 0 END"),
        )
        .and_where(Expr::col(Alias::new("risk_score")).gt(0))
        .and_where(
            Expr::col(Alias::new("last_anomaly_at"))
                .is_null()
                .or(Expr::col(Alias::new("last_anomaly_at")).lt(anomaly_cutoff)),
        )
        .to_owned();
    Ok(database.execute(&query).await?.rows_affected())
}
