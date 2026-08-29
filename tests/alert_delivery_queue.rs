#![allow(clippy::unwrap_used)]

use sea_orm::{
    ConnectionTrait, DatabaseConnection, QueryResult,
    sea_query::{Alias, Expr, ExprTrait, Query, Value},
};
use sonde::{
    database::{self, alert_delivery_repo, alert_repo, app_repo, query::insert, telemetry_repo},
    domain::{
        alert::{AlertExpression, AlertSource, Comparison},
        telemetry::{Attributes, EventInput},
    },
    services::alerts,
};

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

    let expression = AlertExpression {
        source: AlertSource::EventCount,
        operator: Comparison::GreaterOrEqual,
        threshold: 1.0,
        window_minutes: 5,
        consecutive_hits: 1,
        filters: Vec::new(),
    };
    let query = serde_json::to_value(&expression).unwrap();
    let rule_id = alert_repo::create_rule(
        &database,
        alert_repo::NewRule {
            application_id: &application_id,
            name: "startup spike",
            source_kind: "event_count",
            query: &query,
            window_minutes: 5,
            cooldown_seconds: 300,
        },
    )
    .await
    .unwrap();
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
async fn restored_legacy_pending_delivery_without_payload_is_failed_not_stuck() {
    let database = database::connect("sqlite::memory:").await.unwrap();
    database::migrate(&database).await.unwrap();
    let (application_id, _) = app_repo::create_application(&database, "Legacy", "legacy", None)
        .await
        .unwrap();
    let expression = AlertExpression {
        source: AlertSource::EventCount,
        operator: Comparison::GreaterOrEqual,
        threshold: 1.0,
        window_minutes: 5,
        consecutive_hits: 1,
        filters: Vec::new(),
    };
    let query = serde_json::to_value(&expression).unwrap();
    let rule_id = alert_repo::create_rule(
        &database,
        alert_repo::NewRule {
            application_id: &application_id,
            name: "legacy pending",
            source_kind: "event_count",
            query: &query,
            window_minutes: 5,
            cooldown_seconds: 300,
        },
    )
    .await
    .unwrap();
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
            0_i64.into(),
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
