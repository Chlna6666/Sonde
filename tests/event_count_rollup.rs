#![allow(clippy::unwrap_used)]

use sonde::{
    database::{self, event_count_repo, rollup_repo, telemetry_repo},
    domain::telemetry::{Attributes, EventInput},
};

fn event(timestamp: i64, key: &str) -> EventInput {
    EventInput {
        name: "application.start".into(),
        timestamp: Some(timestamp),
        anonymous_id: Some(format!("user-{key}")),
        session_id: Some(format!("session-{key}")),
        app_version: Some("1.0.0".into()),
        launcher_version: Some("1.0.0".into()),
        os: Some("test".into()),
        idempotency_key: Some(key.into()),
        attributes: Attributes::new(),
    }
}

async fn flush_daily_rollups(database: &sea_orm::DatabaseConnection) {
    loop {
        let dirty = rollup_repo::list_dirty_days(database, 128, i64::MAX)
            .await
            .unwrap();
        if dirty.is_empty() {
            break;
        }
        for item in dirty {
            let _ = rollup_repo::recompute_claimed_day(database, item)
                .await
                .unwrap();
        }
    }
}

#[tokio::test]
async fn event_count_hybrid_matches_exact_windows_and_dirty_fallback() {
    let database = database::connect("sqlite::memory:").await.unwrap();
    database::migrate(&database).await.unwrap();

    let app_a_prod = telemetry_repo::TelemetryScope {
        application_id: "app-a".into(),
        environment_id: "prod".into(),
    };
    let app_a_beta = telemetry_repo::TelemetryScope {
        application_id: "app-a".into(),
        environment_id: "beta".into(),
    };
    let app_b_prod = telemetry_repo::TelemetryScope {
        application_id: "app-b".into(),
        environment_id: "prod".into(),
    };
    let day1 = chrono::NaiveDate::from_ymd_opt(2026, 8, 20)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis();
    let day2 = day1 + 86_400_000;
    let day3 = day2 + 86_400_000;

    telemetry_repo::insert_events(
        &database,
        &app_a_prod,
        &[
            event(day1 + 1_000, "a1"),
            event(day1 + 20_000, "a2"),
            event(day2 + 1_000, "a3"),
            event(day2 + 40_000, "a4"),
        ],
    )
    .await
    .unwrap();
    telemetry_repo::insert_events(
        &database,
        &app_a_beta,
        &[event(day2 + 2_000, "ab1")],
    )
    .await
    .unwrap();
    telemetry_repo::insert_events(
        &database,
        &app_b_prod,
        &[event(day1 + 3_000, "b1"), event(day2 + 3_000, "b2")],
    )
    .await
    .unwrap();

    rollup_repo::seed_historical_dirty_days_once(&database)
        .await
        .unwrap();
    flush_daily_rollups(&database).await;

    assert_eq!(
        event_count_repo::event_count_hybrid(
            &database,
            Some("app-a"),
            Some("prod"),
            Some(day1),
            Some(day3),
        )
        .await
        .unwrap(),
        4
    );
    assert_eq!(
        event_count_repo::event_count_hybrid(
            &database,
            Some("app-a"),
            None,
            Some(day1),
            Some(day3),
        )
        .await
        .unwrap(),
        5
    );
    assert_eq!(
        event_count_repo::event_count_hybrid(&database, None, None, Some(day1), Some(day3))
            .await
            .unwrap(),
        7
    );

    // Both boundaries are partial. Only a2 and a3 fall inside this exact interval.
    assert_eq!(
        event_count_repo::event_count_hybrid(
            &database,
            Some("app-a"),
            Some("prod"),
            Some(day1 + 10_000),
            Some(day2 + 10_000),
        )
        .await
        .unwrap(),
        2
    );

    // A fresh event makes the day dirty. The cached rollup is stale, but the hybrid result must be
    // authoritative immediately without waiting for the background worker.
    telemetry_repo::insert_events(
        &database,
        &app_a_prod,
        &[event(day2 + 50_000, "a5")],
    )
    .await
    .unwrap();
    assert_eq!(
        event_count_repo::event_count_hybrid(
            &database,
            Some("app-a"),
            Some("prod"),
            Some(day1),
            Some(day3),
        )
        .await
        .unwrap(),
        5
    );
    assert_eq!(
        event_count_repo::event_count_hybrid(&database, None, None, Some(day1), Some(day3))
            .await
            .unwrap(),
        8
    );
}
