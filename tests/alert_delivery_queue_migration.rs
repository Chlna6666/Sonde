#![allow(clippy::unwrap_used)]

use sea_orm::{ConnectionTrait, sea_query::{Alias, Expr, ExprTrait, Query, Value}};
use sea_orm_migration::MigratorTrait;
use sonde::database::{self, alert_delivery_repo, query::insert};

#[tokio::test]
async fn migration_21_preserves_history_and_makes_legacy_pending_rows_reclaimable() {
    let database = database::connect("sqlite::memory:").await.unwrap();

    // Stop immediately before the durable-delivery migration.
    database::Migrator::up(&database, Some(20)).await.unwrap();
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
        ],
        vec![
            "legacy-delivery".into(),
            "legacy-rule".into(),
            "legacy-channel".into(),
            "pending".into(),
            0_i32.into(),
            Value::String(None),
            Value::BigInt(None),
            100_i64.into(),
        ],
    )
    .await
    .unwrap();

    database::Migrator::up(&database, Some(1)).await.unwrap();

    let row = database
        .query_one(
            &Query::select()
                .columns(["status", "payload_json", "created_at"].map(Alias::new))
                .from(Alias::new("alert_deliveries"))
                .and_where(Expr::col(Alias::new("id")).eq("legacy-delivery"))
                .limit(1)
                .to_owned(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.try_get::<String>("", "status").unwrap(), "pending");
    assert_eq!(row.try_get::<Option<String>>("", "payload_json").unwrap(), None);
    assert_eq!(row.try_get::<i64>("", "created_at").unwrap(), 100);

    let due = alert_delivery_repo::list_due(&database, 1, 10)
        .await
        .unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].id, "legacy-delivery");
    assert!(due[0].payload_json.is_empty());
}
