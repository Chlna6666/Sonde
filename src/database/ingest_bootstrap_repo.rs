use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr, SqlErr, TransactionTrait,
    sea_query::{Alias, Expr, ExprTrait, Query},
};

use super::query;

pub async fn charge_window(
    database: &impl ConnectionTrait,
    bucket_key: &str,
    cost: i64,
    limit: i64,
    expires_at: i64,
) -> Result<bool, DbErr> {
    if cost <= 0 {
        return Ok(true);
    }
    if limit <= 0 || cost > limit {
        return Ok(false);
    }

    if try_update_window(database, bucket_key, cost, limit).await? {
        return Ok(true);
    }

    let now = chrono::Utc::now().timestamp_millis();
    match query::insert(
        database,
        "ingest_rate_windows",
        &["bucket_key", "usage_count", "expires_at", "created_at"],
        vec![
            bucket_key.to_owned().into(),
            cost.into(),
            expires_at.into(),
            now.into(),
        ],
    )
    .await
    {
        Ok(_) => Ok(true),
        Err(error) if is_unique_violation(&error) => {
            try_update_window(database, bucket_key, cost, limit).await
        }
        Err(error) => Err(error),
    }
}

pub async fn record_enrollment_with_budget(
    database: &DatabaseConnection,
    enrollment_key: &str,
    budget_key: &str,
    limit: i64,
    expires_at: i64,
) -> Result<bool, DbErr> {
    let transaction = database.begin().await?;
    let now = chrono::Utc::now().timestamp_millis();
    match query::insert(
        &transaction,
        "ingest_device_enrollments",
        &["enrollment_key", "expires_at", "created_at"],
        vec![
            enrollment_key.to_owned().into(),
            expires_at.into(),
            now.into(),
        ],
    )
    .await
    {
        Ok(_) => {}
        Err(error) if is_unique_violation(&error) => {
            transaction.rollback().await?;
            return Ok(true);
        }
        Err(error) => {
            transaction.rollback().await?;
            return Err(error);
        }
    }

    if charge_window(&transaction, budget_key, 1, limit, expires_at).await? {
        transaction.commit().await?;
        Ok(true)
    } else {
        transaction.rollback().await?;
        Ok(false)
    }
}

pub async fn cleanup_expired(
    database: &impl ConnectionTrait,
    now: i64,
) -> Result<u64, DbErr> {
    let mut removed = 0_u64;
    for table in ["ingest_device_enrollments", "ingest_rate_windows"] {
        let delete = Query::delete()
            .from_table(Alias::new(table))
            .and_where(Expr::col(Alias::new("expires_at")).lte(now))
            .to_owned();
        removed = removed.saturating_add(database.execute(&delete).await?.rows_affected());
    }
    Ok(removed)
}

async fn try_update_window(
    database: &impl ConnectionTrait,
    bucket_key: &str,
    cost: i64,
    limit: i64,
) -> Result<bool, DbErr> {
    let max_before_charge = limit.saturating_sub(cost);
    let update = Query::update()
        .table(Alias::new("ingest_rate_windows"))
        .value(
            Alias::new("usage_count"),
            Expr::col(Alias::new("usage_count")).add(cost),
        )
        .and_where(Expr::col(Alias::new("bucket_key")).eq(bucket_key))
        .and_where(Expr::col(Alias::new("usage_count")).lte(max_before_charge))
        .to_owned();
    Ok(database.execute(&update).await?.rows_affected() == 1)
}

fn is_unique_violation(error: &DbErr) -> bool {
    matches!(
        error.sql_err(),
        Some(SqlErr::UniqueConstraintViolation(_))
    )
}
