use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr,
    sea_query::{Alias, Expr, ExprTrait, Query, Value},
};

use super::query::insert_batch_ignore_conflicts;

pub async fn try_acquire_or_renew(
    database: &DatabaseConnection,
    name: &str,
    holder_id: &str,
    ttl_millis: i64,
) -> Result<bool, DbErr> {
    let now = chrono::Utc::now().timestamp_millis();
    let lease_until = now.saturating_add(ttl_millis.max(1_000));

    if update_existing(database, name, holder_id, now, lease_until).await? {
        return Ok(true);
    }

    let inserted = insert_batch_ignore_conflicts(
        database,
        "job_leases",
        &["name", "holder_id", "lease_until", "updated_at"],
        vec![vec![
            Value::from(name.to_owned()),
            Value::from(holder_id.to_owned()),
            Value::from(lease_until),
            Value::from(now),
        ]],
        "name",
        "name",
    )
    .await?;
    if inserted > 0 {
        return Ok(true);
    }

    // A competing instance may have inserted an already-expired row between our UPDATE and INSERT.
    // Retrying the same conditional UPDATE keeps acquisition atomic without backend-specific locks.
    update_existing(database, name, holder_id, now, lease_until).await
}

async fn update_existing(
    database: &DatabaseConnection,
    name: &str,
    holder_id: &str,
    now: i64,
    lease_until: i64,
) -> Result<bool, DbErr> {
    let update = Query::update()
        .table(Alias::new("job_leases"))
        .value(Alias::new("holder_id"), holder_id)
        .value(Alias::new("lease_until"), lease_until)
        .value(Alias::new("updated_at"), now)
        .and_where(Expr::col(Alias::new("name")).eq(name))
        .and_where(
            Expr::col(Alias::new("lease_until"))
                .lte(now)
                .or(Expr::col(Alias::new("holder_id")).eq(holder_id)),
        )
        .to_owned();
    Ok(database.execute(&update).await?.rows_affected() == 1)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::database;

    #[tokio::test]
    async fn only_current_holder_can_renew_live_lease() {
        let database = database::connect("sqlite::memory:").await.unwrap();
        database::migrate(&database).await.unwrap();

        assert!(super::try_acquire_or_renew(&database, "alerts", "node-a", 60_000)
            .await
            .unwrap());
        assert!(!super::try_acquire_or_renew(&database, "alerts", "node-b", 60_000)
            .await
            .unwrap());
        assert!(super::try_acquire_or_renew(&database, "alerts", "node-a", 60_000)
            .await
            .unwrap());
    }
}
