#![allow(clippy::unwrap_used)]

use sea_orm::{ConnectionTrait, sea_query::{Alias, Expr, ExprTrait, Query, Value}};
use sonde::database::{self, alert_delivery_repo, query::insert};

#[tokio::test]
async fn terminal_history_pruning_never_deletes_pending_deliveries() {
    let database = database::connect("sqlite::memory:").await.unwrap();
    database::migrate(&database).await.unwrap();

    for (id, status, created_at) in [
        ("old-delivered", "delivered", 10_i64),
        ("old-failed", "failed", 20_i64),
        ("old-cancelled", "cancelled", 30_i64),
        ("old-pending", "pending", 40_i64),
        ("new-delivered", "delivered", 200_i64),
    ] {
        insert(
            &database,
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
            vec![
                id.into(),
                "rule".into(),
                "channel".into(),
                status.into(),
                1_i32.into(),
                Value::String(None),
                Value::BigInt(None),
                created_at.into(),
                if status == "pending" {
                    "{\"message\":\"pending\"}".into()
                } else {
                    Value::String(None)
                },
            ],
        )
        .await
        .unwrap();
    }

    assert_eq!(alert_delivery_repo::prune_terminal_before(&database, 100, 2).await.unwrap(), 2);
    assert_eq!(ids(&database).await, vec!["new-delivered", "old-cancelled", "old-pending"]);

    assert_eq!(alert_delivery_repo::prune_terminal_before(&database, 100, 10).await.unwrap(), 1);
    assert_eq!(ids(&database).await, vec!["new-delivered", "old-pending"]);
}

async fn ids(database: &sea_orm::DatabaseConnection) -> Vec<String> {
    database
        .query_all(
            &Query::select()
                .column(Alias::new("id"))
                .from(Alias::new("alert_deliveries"))
                .and_where(Expr::col(Alias::new("id")).is_not_null())
                .order_by(Alias::new("id"), sea_orm::Order::Asc)
                .to_owned(),
        )
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.try_get("", "id").unwrap())
        .collect()
}
