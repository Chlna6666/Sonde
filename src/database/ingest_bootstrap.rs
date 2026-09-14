use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, DbErr, SqlErr, TransactionTrait,
    sea_query::{Alias, Expr, ExprTrait, LockType, Query},
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
    if limit <= 0 {
        return Ok(false);
    }
    ensure_window(database, budget_key, expires_at).await?;

    let backend = database.get_database_backend();
    let transaction = database.begin().await?;
    let usage = lock_and_load_window(&transaction, backend, budget_key)
        .await?
        .ok_or_else(|| DbErr::Custom("ingest enrollment budget row disappeared".into()))?;

    if enrollment_exists(&transaction, enrollment_key).await? {
        transaction.commit().await?;
        return Ok(true);
    }
    if usage >= limit {
        transaction.rollback().await?;
        return Ok(false);
    }

    let now = chrono::Utc::now().timestamp_millis();
    query::insert(
        &transaction,
        "ingest_device_enrollments",
        &["enrollment_key", "expires_at", "created_at"],
        vec![
            enrollment_key.to_owned().into(),
            expires_at.into(),
            now.into(),
        ],
    )
    .await?;

    let update = Query::update()
        .table(Alias::new("ingest_rate_windows"))
        .value(Alias::new("usage_count"), usage.saturating_add(1))
        .and_where(Expr::col(Alias::new("bucket_key")).eq(budget_key))
        .to_owned();
    transaction.execute(&update).await?;
    transaction.commit().await?;
    Ok(true)
}

pub async fn cleanup_expired(database: &impl ConnectionTrait, now: i64) -> Result<u64, DbErr> {
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

async fn ensure_window(
    database: &DatabaseConnection,
    bucket_key: &str,
    expires_at: i64,
) -> Result<(), DbErr> {
    let now = chrono::Utc::now().timestamp_millis();
    match query::insert(
        database,
        "ingest_rate_windows",
        &["bucket_key", "usage_count", "expires_at", "created_at"],
        vec![
            bucket_key.to_owned().into(),
            0_i64.into(),
            expires_at.into(),
            now.into(),
        ],
    )
    .await
    {
        Ok(_) => Ok(()),
        Err(error) if is_unique_violation(&error) => Ok(()),
        Err(error) => Err(error),
    }
}

async fn lock_and_load_window(
    database: &impl ConnectionTrait,
    backend: DbBackend,
    bucket_key: &str,
) -> Result<Option<i64>, DbErr> {
    if backend == DbBackend::Sqlite {
        // Force the deferred SQLite transaction to acquire its write lock before the read below.
        let touch = Query::update()
            .table(Alias::new("ingest_rate_windows"))
            .value(
                Alias::new("usage_count"),
                Expr::col(Alias::new("usage_count")),
            )
            .and_where(Expr::col(Alias::new("bucket_key")).eq(bucket_key))
            .to_owned();
        database.execute(&touch).await?;
    }

    let mut query = Query::select();
    query
        .column(Alias::new("usage_count"))
        .from(Alias::new("ingest_rate_windows"))
        .and_where(Expr::col(Alias::new("bucket_key")).eq(bucket_key))
        .limit(1);
    if backend != DbBackend::Sqlite {
        query.lock(LockType::Update);
    }
    database
        .query_one(&query.to_owned())
        .await?
        .map(|row| row.try_get::<i64>("", "usage_count"))
        .transpose()
}

async fn enrollment_exists(
    database: &impl ConnectionTrait,
    enrollment_key: &str,
) -> Result<bool, DbErr> {
    let query = Query::select()
        .expr(Expr::value(1))
        .from(Alias::new("ingest_device_enrollments"))
        .and_where(Expr::col(Alias::new("enrollment_key")).eq(enrollment_key))
        .limit(1)
        .to_owned();
    Ok(database.query_one(&query).await?.is_some())
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
    matches!(error.sql_err(), Some(SqlErr::UniqueConstraintViolation(_)))
}
