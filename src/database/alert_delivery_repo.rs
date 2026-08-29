use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr, TransactionTrait,
    sea_query::{Alias, Expr, ExprTrait, Order, Query, Value},
};
use uuid::Uuid;

use super::query::insert_batch;

const MAX_DELIVERY_PAYLOAD_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug)]
pub struct PendingDelivery {
    pub id: String,
    pub rule_id: String,
    pub channel_id: String,
    pub attempts: i32,
    pub payload_json: String,
}

/// Persist an alert state transition and every outbound delivery in one transaction.
///
/// Once this commits, a process crash can delay delivery but cannot lose it. The queue deliberately
/// provides at-least-once semantics because an HTTP receiver cannot participate in our database
/// transaction; a crash after the remote endpoint accepted a request but before the success update
/// may therefore cause one retry.
pub async fn persist_transition(
    database: &DatabaseConnection,
    rule_id: &str,
    state: &str,
    evaluated_at: i64,
    channel_ids: &[String],
    payload: &serde_json::Value,
) -> Result<usize, DbErr> {
    let payload_json = serde_json::to_string(payload)
        .map_err(|error| DbErr::Custom(format!("alert payload serialization failed: {error}")))?;
    if payload_json.len() > MAX_DELIVERY_PAYLOAD_BYTES {
        return Err(DbErr::Custom("alert delivery payload exceeds 64 KiB".into()));
    }

    let transaction = database.begin().await?;
    let update = Query::update()
        .table(Alias::new("alert_rules"))
        .value(Alias::new("last_state"), state)
        .value(Alias::new("last_evaluated_at"), evaluated_at)
        .and_where(Expr::col(Alias::new("id")).eq(rule_id))
        .to_owned();
    if transaction.execute(&update).await?.rows_affected() != 1 {
        transaction.rollback().await?;
        return Err(DbErr::Custom("alert rule disappeared during transition".into()));
    }

    if !channel_ids.is_empty() {
        let rows = channel_ids
            .iter()
            .map(|channel_id| {
                vec![
                    Value::from(Uuid::now_v7().to_string()),
                    Value::from(rule_id.to_owned()),
                    Value::from(channel_id.clone()),
                    Value::from("pending"),
                    Value::from(0_i32),
                    Value::String(None),
                    Value::from(evaluated_at),
                    Value::from(evaluated_at),
                    Value::from(payload_json.clone()),
                ]
            })
            .collect::<Vec<_>>();
        insert_batch(
            &transaction,
            "alert_deliveries",
            &[
                "id",
                "rule_id",
                "channel_id",
                "status",
                "attempts",
                "last_error",
                "next_attempt_at",
                "created_at",
                "payload_json",
            ],
            rows,
        )
        .await?;
    }

    transaction.commit().await?;
    Ok(channel_ids.len())
}

pub async fn list_due(
    database: &DatabaseConnection,
    now: i64,
    limit: u64,
) -> Result<Vec<PendingDelivery>, DbErr> {
    let query = Query::select()
        .columns(
            [
                "id",
                "rule_id",
                "channel_id",
                "attempts",
                "payload_json",
            ]
            .map(Alias::new),
        )
        .from(Alias::new("alert_deliveries"))
        .and_where(Expr::col(Alias::new("status")).eq("pending"))
        .and_where(Expr::col(Alias::new("next_attempt_at")).lte(now))
        .and_where(Expr::col(Alias::new("payload_json")).is_not_null())
        .order_by(Alias::new("next_attempt_at"), Order::Asc)
        .order_by(Alias::new("created_at"), Order::Asc)
        .limit(limit.max(1))
        .to_owned();

    database
        .query_all(&query)
        .await?
        .into_iter()
        .map(|row| {
            let attempts = row
                .try_get::<i32>("", "attempts")
                .or_else(|_| row.try_get::<i64>("", "attempts").map(|value| value as i32))?;
            Ok(PendingDelivery {
                id: row.try_get("", "id")?,
                rule_id: row.try_get("", "rule_id")?,
                channel_id: row.try_get("", "channel_id")?,
                attempts,
                payload_json: row.try_get("", "payload_json")?,
            })
        })
        .collect()
}

pub async fn mark_delivered(
    database: &DatabaseConnection,
    id: &str,
    expected_attempts: i32,
) -> Result<bool, DbErr> {
    finish_attempt(
        database,
        id,
        expected_attempts,
        expected_attempts.saturating_add(1),
        "delivered",
        None,
        None,
    )
    .await
}

pub async fn reschedule(
    database: &DatabaseConnection,
    id: &str,
    expected_attempts: i32,
    last_error: &str,
    next_attempt_at: i64,
) -> Result<bool, DbErr> {
    finish_attempt(
        database,
        id,
        expected_attempts,
        expected_attempts.saturating_add(1),
        "pending",
        Some(last_error),
        Some(next_attempt_at),
    )
    .await
}

pub async fn mark_failed(
    database: &DatabaseConnection,
    id: &str,
    expected_attempts: i32,
    attempted: bool,
    last_error: &str,
) -> Result<bool, DbErr> {
    let attempts = if attempted {
        expected_attempts.saturating_add(1)
    } else {
        expected_attempts
    };
    finish_attempt(
        database,
        id,
        expected_attempts,
        attempts,
        "failed",
        Some(last_error),
        None,
    )
    .await
}

async fn finish_attempt(
    database: &DatabaseConnection,
    id: &str,
    expected_attempts: i32,
    attempts: i32,
    status: &str,
    last_error: Option<&str>,
    next_attempt_at: Option<i64>,
) -> Result<bool, DbErr> {
    let update = Query::update()
        .table(Alias::new("alert_deliveries"))
        .value(Alias::new("status"), status)
        .value(Alias::new("attempts"), attempts)
        .value(
            Alias::new("last_error"),
            last_error
                .map(str::to_owned)
                .map(Value::from)
                .unwrap_or(Value::String(None)),
        )
        .value(
            Alias::new("next_attempt_at"),
            next_attempt_at
                .map(Value::from)
                .unwrap_or(Value::BigInt(None)),
        )
        .and_where(Expr::col(Alias::new("id")).eq(id))
        .and_where(Expr::col(Alias::new("status")).eq("pending"))
        .and_where(Expr::col(Alias::new("attempts")).eq(expected_attempts))
        .to_owned();
    Ok(database.execute(&update).await?.rows_affected() == 1)
}

#[cfg(test)]
mod tests {
    #[test]
    fn retry_attempts_are_monotonic() {
        assert_eq!(0_i32.saturating_add(1), 1);
        assert_eq!(2_i32.saturating_add(1), 3);
    }
}
