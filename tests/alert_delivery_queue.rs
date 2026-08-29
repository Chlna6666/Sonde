#![allow(clippy::unwrap_used)]

use sea_orm::{
    ConnectionTrait, DatabaseConnection, QueryResult,
    sea_query::{Alias, Expr, ExprTrait, Func, Query, Value},
};
use sonde::{
    database::{
        self, alert_delivery_repo, alert_repo, app_repo, application_delete_repo, query::insert,
        telemetry_repo,
    },
    domain::{
        alert::{AlertExpression, AlertSource, Comparison},
        telemetry::{Attributes, EventInput},
    },
    services::alerts,
};

fn event_count_expression() -> AlertExpression {
    AlertExpression {
        source: AlertSource::EventCount,
        operator: Comparison::GreaterOrEqual,
        threshold: 1.0,
        window_minutes: 5,
        consecutive_hits: 1,
        filters: Vec::new(),
    }
}

async fn create_rule(database: &DatabaseConnection, application_id: &str, name: &str) -> String {
    let expression = event_count_expression();
    let query = serde_json::to_value(&expression).unwrap();
    alert_repo::create_rule(
        database,
        alert_repo::NewRule {
            application_id,
            name,
            source_kind: "event_count",
            query: &query,
            window_minutes: 5,
            cooldown_seconds: 300,
        },
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn evaluator_persists_delivery_and_worker_retries_without_process_state() {
    let database = database::connect("sqlite::memory:").await.unwrap();
    database::migrate(&database).await.unwrap();
    let (application_id, environment_id) =
        app_repo::create_application(&database, "Alerts", "alerts", None)
            .await
            .unwrap();

    telemetry_repo::insert_events(
        &database,
        &telemetry_repo::TelemetryScope {
            application_id: application_id.clone(),
            environment_id,
        },
        &[EventInput {
            name: "application.start".into(),
            timestamp: Some(chrono::Utc::now().timestamp_millis()),
            anonymous_id: None,
            session_id: None,
            app_version: None,
            launcher_version: None,
            os: None,
            idempotency_key: None,
            attributes: Attributes::new(),
        }],
    )
    .await
    .unwrap();

    let rule_id = create_rule(&database, &application_id, "startup spike").await;
    let channel_id = alert_repo::create_channel(
        &database,
        "intentionally unsupported",
        "unsupported-test-channel",
        &serde_json::json!({}),
        true,
    )
    .await
    .unwrap();

    assert_eq!(alerts::evaluate_all_rules(&database).await.unwrap(), 1);
    let rule = alert_repo::get_rule(&database, &rule_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(rule.last_state, "firing");

    let delivery = only_delivery(&database).await;
    assert_eq!(delivery.1, "pending");
    assert_eq!(delivery.2, 0);
    assert!(delivery.3.is_some());
    assert_eq!(delivery.4.as_deref(), Some(channel_id.as_str()));

    // Each invocation is intentionally stateless. A service restart between any of these calls does
    // not lose the retry schedule because attempts and next_attempt_at are authoritative DB state.
    assert_eq!(alerts::process_due_deliveries(&database, 4).await.unwrap(), 1);
    let delivery = only_delivery(&database).await;
    assert_eq!(delivery.1, "pending");
    assert_eq!(delivery.2, 1);
    assert!(delivery.3.is_some_and(|next| next > chrono::Utc::now().timestamp_millis()));

    force_due(&database, &delivery.0).await;
    assert_eq!(alerts::process_due_deliveries(&database, 4).await.unwrap(), 1);
    let delivery = only_delivery(&database).await;
    assert_eq!(delivery.1, "pending");
    assert_eq!(delivery.2, 2);

    force_due(&database, &delivery.0).await;
    assert_eq!(alerts::process_due_deliveries(&database, 4).await.unwrap(), 1);
    let delivery = only_delivery(&database).await;
    assert_eq!(delivery.1, "failed");
    assert_eq!(delivery.2, 3);
    assert!(delivery.3.is_none());
    assert!(delivery.5.as_deref().is_some_and(|error| error.contains("Unsupported")));
}

#[tokio::test]
async fn restored_legacy_pending_delivery_without_schedule_or_payload_is_reclaimed() {
    let database = database::connect("sqlite::memory:").await.unwrap();
    database::migrate(&database).await.unwrap();
    let (application_id, _) = app_repo::create_application(&database, "Legacy", "legacy", None)
        .await
        .unwrap();
    let rule_id = create_rule(&database, &application_id, "legacy pending").await;
    let channel_id = alert_repo::create_channel(
        &database,
        "legacy channel",
        "unsupported-test-channel",
        &serde_json::json!({}),
        true,
    )
    .await
    .unwrap();

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
            "legacy-pending".into(),
            rule_id.into(),
            channel_id.into(),
            "pending".into(),
            0_i32.into(),
            Value::String(None),
            Value::BigInt(None),
            0_i64.into(),
            Value::String(None),
        ],
    )
    .await
    .unwrap();

    let due = alert_delivery_repo::list_due(&database, 1, 4)
        .await
        .unwrap();
    assert_eq!(due.len(), 1);
    assert!(due[0].payload_json.is_empty());

    assert_eq!(alerts::process_due_deliveries(&database, 4).await.unwrap(), 1);
    let row = database
        .query_one(
            &Query::select()
                .columns(["status", "attempts", "last_error"].map(Alias::new))
                .from(Alias::new("alert_deliveries"))
                .and_where(Expr::col(Alias::new("id")).eq("legacy-pending"))
                .limit(1)
                .to_owned(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.try_get::<String>("", "status").unwrap(), "failed");
    assert_eq!(read_i32(&row, "attempts"), 0);
    assert!(row
        .try_get::<String>("", "last_error")
        .unwrap()
        .contains("invalid persisted alert payload"));
}

#[tokio::test]
async fn disabling_rule_or_channel_cancels_pending_deliveries() {
    let database = database::connect("sqlite::memory:").await.unwrap();
    database::migrate(&database).await.unwrap();
    let (application_id, _) = app_repo::create_application(&database, "Cancel", "cancel", None)
        .await
        .unwrap();
    let rule_id = create_rule(&database, &application_id, "cancel rule").await;
    let channel_id = alert_repo::create_channel(
        &database,
        "cancel channel",
        "unsupported-test-channel",
        &serde_json::json!({}),
        true,
    )
    .await
    .unwrap();
    let payload = serde_json::json!({"status":"firing","message":"test"});
    alert_delivery_repo::persist_transition(
        &database,
        &rule_id,
        "firing",
        100,
        std::slice::from_ref(&channel_id),
        &payload,
    )
    .await
    .unwrap();

    let expression = event_count_expression();
    let query = serde_json::to_value(&expression).unwrap();
    alert_repo::update_rule(
        &database,
        &rule_id,
        alert_repo::UpdateRule {
            name: "cancel rule",
            enabled: false,
            source_kind: "event_count",
            query: &query,
            window_minutes: 5,
            cooldown_seconds: 300,
        },
    )
    .await
    .unwrap();
    let first = only_delivery(&database).await;
    assert_eq!(first.1, "cancelled");
    assert_eq!(first.2, 0);
    assert!(first.3.is_none());
    assert!(first.5.as_deref().is_some_and(|error| error.contains("disabled")));
    assert!(alert_delivery_repo::list_due(&database, i64::MAX, 4)
        .await
        .unwrap()
        .is_empty());

    // Re-enable the rule and enqueue another row, then disable the channel. Only the new pending row
    // is cancelled; the earlier cancellation remains immutable delivery history.
    alert_repo::update_rule(
        &database,
        &rule_id,
        alert_repo::UpdateRule {
            name: "cancel rule",
            enabled: true,
            source_kind: "event_count",
            query: &query,
            window_minutes: 5,
            cooldown_seconds: 300,
        },
    )
    .await
    .unwrap();
    alert_delivery_repo::persist_transition(
        &database,
        &rule_id,
        "firing",
        200,
        std::slice::from_ref(&channel_id),
        &payload,
    )
    .await
    .unwrap();
    alert_repo::update_channel(
        &database,
        &channel_id,
        "cancel channel",
        "unsupported-test-channel",
        &serde_json::json!({}),
        false,
    )
    .await
    .unwrap();

    let statuses = delivery_statuses(&database).await;
    assert_eq!(statuses, vec!["cancelled", "cancelled"]);
    assert!(alert_delivery_repo::list_due(&database, i64::MAX, 4)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn deleting_application_removes_its_pending_delivery_queue() {
    let database = database::connect("sqlite::memory:").await.unwrap();
    database::migrate(&database).await.unwrap();
    let (application_id, _) = app_repo::create_application(&database, "Delete", "delete", None)
        .await
        .unwrap();
    let rule_id = create_rule(&database, &application_id, "delete rule").await;
    let channel_id = alert_repo::create_channel(
        &database,
        "delete channel",
        "unsupported-test-channel",
        &serde_json::json!({}),
        true,
    )
    .await
    .unwrap();
    alert_delivery_repo::persist_transition(
        &database,
        &rule_id,
        "firing",
        100,
        &[channel_id],
        &serde_json::json!({"status":"firing","message":"test"}),
    )
    .await
    .unwrap();

    assert_eq!(delivery_count(&database).await, 1);
    application_delete_repo::delete_application_exact(&database, &application_id)
        .await
        .unwrap();
    assert_eq!(delivery_count(&database).await, 0);
}

async fn only_delivery(
    database: &DatabaseConnection,
) -> (String, String, i32, Option<i64>, Option<String>, Option<String>) {
    let row = database
        .query_one(
            &Query::select()
                .columns(
                    [
                        "id",
                        "status",
                        "attempts",
                        "next_attempt_at",
                        "channel_id",
                        "last_error",
                    ]
                    .map(Alias::new),
                )
                .from(Alias::new("alert_deliveries"))
                .order_by(Alias::new("created_at"), sea_orm::Order::Desc)
                .limit(1)
                .to_owned(),
        )
        .await
        .unwrap()
        .unwrap();
    (
        row.try_get("", "id").unwrap(),
        row.try_get("", "status").unwrap(),
        read_i32(&row, "attempts"),
        row.try_get("", "next_attempt_at").unwrap(),
        row.try_get("", "channel_id").unwrap(),
        row.try_get("", "last_error").unwrap(),
    )
}

async fn delivery_statuses(database: &DatabaseConnection) -> Vec<String> {
    database
        .query_all(
            &Query::select()
                .column(Alias::new("status"))
                .from(Alias::new("alert_deliveries"))
                .order_by(Alias::new("created_at"), sea_orm::Order::Asc)
                .to_owned(),
        )
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.try_get("", "status").unwrap())
        .collect()
}

async fn delivery_count(database: &DatabaseConnection) -> i64 {
    database
        .query_one(
            &Query::select()
                .expr_as(Func::count(Expr::col(Alias::new("id"))), Alias::new("total"))
                .from(Alias::new("alert_deliveries"))
                .to_owned(),
        )
        .await
        .unwrap()
        .and_then(|row| row.try_get::<i64>("", "total").ok())
        .unwrap_or(0)
}

fn read_i32(row: &QueryResult, column: &str) -> i32 {
    row.try_get::<i32>("", column)
        .unwrap_or_else(|_| row.try_get::<i64>("", column).unwrap() as i32)
}

async fn force_due(database: &DatabaseConnection, id: &str) {
    let update = Query::update()
        .table(Alias::new("alert_deliveries"))
        .value(Alias::new("next_attempt_at"), 0_i64)
        .and_where(Expr::col(Alias::new("id")).eq(id))
        .to_owned();
    database.execute(&update).await.unwrap();
}
