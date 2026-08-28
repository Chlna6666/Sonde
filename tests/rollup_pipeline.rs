#![allow(clippy::unwrap_used)]

use sonde::{
    database::{self, rollup_repo, telemetry_repo},
    domain::telemetry::{Attributes, EventInput},
};

fn event(timestamp: i64, idempotency_key: &str) -> EventInput {
    EventInput {
        name: "application.start".into(),
        timestamp: Some(timestamp),
        anonymous_id: Some("device-a".into()),
        session_id: Some("session-a".into()),
        app_version: Some("1.0.0".into()),
        launcher_version: None,
        os: Some("test".into()),
        idempotency_key: Some(idempotency_key.into()),
        attributes: Attributes::new(),
    }
}

#[tokio::test]
async fn rollup_tracks_committed_ingest_and_dirty_raw_fallback() {
    let database = database::connect("sqlite::memory:").await.unwrap();
    database::migrate(&database).await.unwrap();
    let scope = telemetry_repo::TelemetryScope {
        application_id: "app-a".into(),
        environment_id: "prod".into(),
    };
    let timestamp = chrono::Utc::now().timestamp_millis();
    let day = chrono::DateTime::from_timestamp_millis(timestamp)
        .unwrap()
        .format("%Y-%m-%d")
        .to_string();

    telemetry_repo::insert_events(&database, &scope, &[event(timestamp, "evt-1")])
        .await
        .unwrap();
    rollup_repo::seed_historical_dirty_days_once(&database)
        .await
        .unwrap();

    let dirty = rollup_repo::list_dirty_days(&database, 16, i64::MAX)
        .await
        .unwrap();
    let environment_dirty = dirty
        .into_iter()
        .find(|item| item.application_id == "app-a" && item.environment_id == "prod")
        .unwrap();
    assert!(rollup_repo::recompute_claimed_day(&database, environment_dirty)
        .await
        .unwrap());

    let initial = rollup_repo::application_event_trend_hybrid(
        &database,
        "app-a",
        Some("prod"),
        &day,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(initial.len(), 1);
    assert_eq!(initial[0].events, 1);
    assert_eq!(initial[0].users, 1);

    // New committed telemetry marks the same day dirty. The hybrid read must use raw data for that
    // day immediately instead of serving the stale rollup while the background worker catches up.
    telemetry_repo::insert_events(&database, &scope, &[event(timestamp + 1, "evt-2")])
        .await
        .unwrap();
    telemetry_repo::insert_events(&database, &scope, &[event(timestamp + 2, "evt-3")])
        .await
        .unwrap();

    let refreshed = rollup_repo::application_event_trend_hybrid(
        &database,
        "app-a",
        Some("prod"),
        &day,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(refreshed.len(), 1);
    assert_eq!(refreshed[0].events, 3);
    assert_eq!(refreshed[0].users, 1);

    let dirty = rollup_repo::list_dirty_days(&database, 16, i64::MAX)
        .await
        .unwrap();
    let environment_dirty = dirty
        .into_iter()
        .find(|item| item.application_id == "app-a" && item.environment_id == "prod")
        .unwrap();
    assert!(environment_dirty.generation >= 2);
}
