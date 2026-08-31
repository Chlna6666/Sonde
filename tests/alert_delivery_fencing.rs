#![allow(clippy::unwrap_used)]

use sea_orm::{
    ConnectionTrait,
    sea_query::{Alias, Expr, ExprTrait, Query},
};
use sonde::database::{self, alert_delivery, alerts as alert_store, applications};

async fn setup() -> (sea_orm::DatabaseConnection, String, String, String) {
    let database = database::connect("sqlite::memory:").await.unwrap();
    database::migrate(&database).await.unwrap();
    let (application_id, _) = applications::create_application(&database, "Fence", "fence", None)
        .await
        .unwrap();
    let query = serde_json::json!({
        "source": "event_count",
        "operator": "greater_or_equal",
        "threshold": 1.0,
        "windowMinutes": 5,
        "consecutiveHits": 1,
        "filters": []
    });
    let rule_id = alert_store::create_rule(
        &database,
        alert_store::NewRule {
            application_id: &application_id,
            name: "fenced rule",
            source_kind: "event_count",
            query: &query,
            window_minutes: 5,
            cooldown_seconds: 300,
        },
    )
    .await
    .unwrap();
    let channel_id = alert_store::create_channel(
        &database,
        "fenced channel",
        "unsupported-test-channel",
        &serde_json::json!({}),
        true,
    )
    .await
    .unwrap();
    (database, application_id, rule_id, channel_id)
}

#[tokio::test]
async fn stale_evaluator_cannot_reactivate_disabled_rule() {
    let (database, _, rule_id, channel_id) = setup().await;
    let query = serde_json::json!({
        "source": "event_count",
        "operator": "greater_or_equal",
        "threshold": 1.0,
        "windowMinutes": 5,
        "consecutiveHits": 1,
        "filters": []
    });
    alert_store::update_rule(
        &database,
        &rule_id,
        alert_store::UpdateRule {
            name: "fenced rule",
            enabled: false,
            source_kind: "event_count",
            query: &query,
            window_minutes: 5,
            cooldown_seconds: 300,
        },
    )
    .await
    .unwrap();

    let queued = alert_delivery::persist_transition(
        &database,
        &rule_id,
        "firing",
        123,
        &[channel_id],
        &serde_json::json!({"status":"firing","message":"stale"}),
    )
    .await
    .unwrap();
    assert_eq!(queued, 0);
    assert_eq!(delivery_count(&database).await, 0);
    let rule = alert_store::get_rule(&database, &rule_id)
        .await
        .unwrap()
        .unwrap();
    assert!(!rule.enabled);
    assert_eq!(rule.last_state, "healthy");
}

#[tokio::test]
async fn enqueue_transaction_filters_channels_disabled_after_evaluator_snapshot() {
    let (database, _, rule_id, channel_id) = setup().await;
    alert_store::update_channel(
        &database,
        &channel_id,
        "fenced channel",
        "unsupported-test-channel",
        &serde_json::json!({}),
        false,
    )
    .await
    .unwrap();

    let queued = alert_delivery::persist_transition(
        &database,
        &rule_id,
        "firing",
        123,
        &[channel_id],
        &serde_json::json!({"status":"firing","message":"stale channel"}),
    )
    .await
    .unwrap();
    assert_eq!(queued, 0);
    assert_eq!(delivery_count(&database).await, 0);
    assert_eq!(
        alert_store::get_rule(&database, &rule_id)
            .await
            .unwrap()
            .unwrap()
            .last_state,
        "firing"
    );
}

#[tokio::test]
async fn terminal_delivery_state_drops_retry_payload() {
    let (database, _, rule_id, channel_id) = setup().await;
    assert_eq!(
        alert_delivery::persist_transition(
            &database,
            &rule_id,
            "firing",
            123,
            &[channel_id],
            &serde_json::json!({"status":"firing","message":"discard me"}),
        )
        .await
        .unwrap(),
        1
    );
    let row = database
        .query_one(
            &Query::select()
                .columns(["id", "payload_json"].map(Alias::new))
                .from(Alias::new("alert_deliveries"))
                .limit(1)
                .to_owned(),
        )
        .await
        .unwrap()
        .unwrap();
    let id: String = row.try_get("", "id").unwrap();
    assert!(row
        .try_get::<Option<String>>("", "payload_json")
        .unwrap()
        .is_some());

    assert!(alert_delivery::mark_failed(&database, &id, 0, true, "test")
        .await
        .unwrap());
    let payload = database
        .query_one(
            &Query::select()
                .column(Alias::new("payload_json"))
                .from(Alias::new("alert_deliveries"))
                .and_where(Expr::col(Alias::new("id")).eq(id))
                .limit(1)
                .to_owned(),
        )
        .await
        .unwrap()
        .unwrap()
        .try_get::<Option<String>>("", "payload_json")
        .unwrap();
    assert!(payload.is_none());
}

async fn delivery_count(database: &sea_orm::DatabaseConnection) -> i64 {
    database
        .query_all(
            &Query::select()
                .column(Alias::new("id"))
                .from(Alias::new("alert_deliveries"))
                .to_owned(),
        )
        .await
        .unwrap()
        .len() as i64
}
