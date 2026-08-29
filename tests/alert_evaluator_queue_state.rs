#![allow(clippy::unwrap_used)]

use sea_orm::{ConnectionTrait, sea_query::{Alias, Expr, Func, Query}};
use sonde::{
    database::{self, alert_repo, app_repo, telemetry_repo},
    domain::{
        alert::{AlertExpression, AlertSource, Comparison},
        telemetry::{Attributes, EventInput},
    },
    services::alerts,
};

async fn setup(consecutive_hits: u16) -> (sea_orm::DatabaseConnection, String) {
    let database = database::connect("sqlite::memory:").await.unwrap();
    database::migrate(&database).await.unwrap();
    let (application_id, environment_id) =
        app_repo::create_application(&database, "Evaluator", "evaluator", None)
            .await
            .unwrap();
    telemetry_repo::insert_events(
        &database,
        &telemetry_repo::TelemetryScope {
            application_id: application_id.clone(),
            environment_id,
        },
        &[EventInput {
            name: "hit".into(),
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
        consecutive_hits,
        filters: Vec::new(),
    };
    let query = serde_json::to_value(expression).unwrap();
    let rule_id = alert_repo::create_rule(
        &database,
        alert_repo::NewRule {
            application_id: &application_id,
            name: "state machine",
            source_kind: "event_count",
            query: &query,
            window_minutes: 5,
            cooldown_seconds: 300,
        },
    )
    .await
    .unwrap();
    alert_repo::create_channel(
        &database,
        "queue only",
        "unsupported-test-channel",
        &serde_json::json!({}),
        true,
    )
    .await
    .unwrap();
    (database, rule_id)
}

#[tokio::test]
async fn firing_rule_does_not_enqueue_again_inside_cooldown() {
    let (database, rule_id) = setup(1).await;
    assert_eq!(alerts::evaluate_all_rules(&database).await.unwrap(), 1);
    assert_eq!(delivery_count(&database).await, 1);
    assert_eq!(
        alert_repo::get_rule(&database, &rule_id)
            .await
            .unwrap()
            .unwrap()
            .last_state,
        "firing"
    );

    assert_eq!(alerts::evaluate_all_rules(&database).await.unwrap(), 1);
    assert_eq!(delivery_count(&database).await, 1);
}

#[tokio::test]
async fn consecutive_hits_only_enqueue_after_threshold_is_reached() {
    let (database, rule_id) = setup(2).await;
    assert_eq!(alerts::evaluate_all_rules(&database).await.unwrap(), 1);
    assert_eq!(delivery_count(&database).await, 0);
    assert_eq!(
        alert_repo::get_rule(&database, &rule_id)
            .await
            .unwrap()
            .unwrap()
            .last_state,
        "pending:1"
    );

    assert_eq!(alerts::evaluate_all_rules(&database).await.unwrap(), 1);
    assert_eq!(delivery_count(&database).await, 1);
    assert_eq!(
        alert_repo::get_rule(&database, &rule_id)
            .await
            .unwrap()
            .unwrap()
            .last_state,
        "firing"
    );
}

async fn delivery_count(database: &sea_orm::DatabaseConnection) -> i64 {
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
