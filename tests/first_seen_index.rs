#![allow(clippy::unwrap_used)]

use sonde::{
    database::{self, first_seen_repo, rollup_repo, telemetry_repo},
    domain::telemetry::{Attributes, EventInput},
};

fn event(timestamp: i64, key: &str, user: &str) -> EventInput {
    EventInput {
        name: "application.start".into(),
        timestamp: Some(timestamp),
        anonymous_id: Some(user.into()),
        session_id: Some(format!("session-{key}")),
        app_version: Some("1.0.0".into()),
        launcher_version: Some("1.0.0".into()),
        os: Some("test".into()),
        idempotency_key: Some(key.into()),
        attributes: Attributes::new(),
    }
}

async fn backfill_all(database: &sea_orm::DatabaseConnection) {
    loop {
        let processed = first_seen_repo::run_backfill_batch(database, 32)
            .await
            .unwrap();
        if processed == 0 {
            break;
        }
    }
    assert!(first_seen_repo::backfill_complete(database).await.unwrap());
}

async fn refresh_and_clear_dirty(
    database: &sea_orm::DatabaseConnection,
    application_id: &str,
    environment_id: &str,
) {
    let dirty = rollup_repo::list_dirty_days(database, 64, i64::MAX)
        .await
        .unwrap()
        .into_iter()
        .find(|item| {
            item.application_id == application_id && item.environment_id == environment_id
        })
        .unwrap();
    assert!(first_seen_repo::refresh_dirty_day(database, &dirty)
        .await
        .unwrap());
    assert!(rollup_repo::recompute_claimed_day(database, dirty)
        .await
        .unwrap());
}

#[tokio::test]
async fn first_seen_index_preserves_scope_and_dirty_fallback_semantics() {
    let database = database::connect("sqlite::memory:").await.unwrap();
    database::migrate(&database).await.unwrap();
    let prod = telemetry_repo::TelemetryScope {
        application_id: "app-a".into(),
        environment_id: "prod".into(),
    };
    let beta = telemetry_repo::TelemetryScope {
        application_id: "app-a".into(),
        environment_id: "beta".into(),
    };
    let other = telemetry_repo::TelemetryScope {
        application_id: "app-b".into(),
        environment_id: "prod".into(),
    };
    let start = chrono::NaiveDate::from_ymd_opt(2026, 8, 20)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis();

    telemetry_repo::insert_events(
        &database,
        &prod,
        &[
            event(start + 1_000, "a-1", "user-a"),
            event(start + 2_000, "a-2", "user-b"),
        ],
    )
    .await
    .unwrap();
    telemetry_repo::insert_events(
        &database,
        &beta,
        &[event(start + 3_000, "a-3", "user-a")],
    )
    .await
    .unwrap();
    telemetry_repo::insert_events(
        &database,
        &other,
        &[event(start + 4_000, "b-1", "user-a")],
    )
    .await
    .unwrap();

    backfill_all(&database).await;

    assert_eq!(
        first_seen_repo::count_new_users_hybrid(
            &database,
            Some("app-a"),
            Some("prod"),
            start,
            None,
        )
        .await
        .unwrap(),
        2
    );
    assert_eq!(
        first_seen_repo::count_new_users_hybrid(
            &database,
            Some("app-a"),
            None,
            start,
            None,
        )
        .await
        .unwrap(),
        2
    );
    assert_eq!(
        first_seen_repo::count_new_users_hybrid(&database, None, None, start, None)
            .await
            .unwrap(),
        2
    );

    // A new event makes the scope dirty. The index is intentionally stale until the worker runs,
    // but the hybrid query must still return the authoritative raw result immediately.
    telemetry_repo::insert_events(
        &database,
        &prod,
        &[event(start + 5_000, "a-4", "user-c")],
    )
    .await
    .unwrap();
    assert_eq!(
        first_seen_repo::count_new_users_hybrid(
            &database,
            Some("app-a"),
            Some("prod"),
            start,
            None,
        )
        .await
        .unwrap(),
        3
    );
    refresh_and_clear_dirty(&database, "app-a", "prod").await;
    assert_eq!(
        first_seen_repo::count_new_users_hybrid(
            &database,
            Some("app-a"),
            Some("prod"),
            start,
            None,
        )
        .await
        .unwrap(),
        3
    );
}

#[tokio::test]
async fn first_seen_index_moves_earlier_for_out_of_order_events() {
    let database = database::connect("sqlite::memory:").await.unwrap();
    database::migrate(&database).await.unwrap();
    let scope = telemetry_repo::TelemetryScope {
        application_id: "app-order".into(),
        environment_id: "prod".into(),
    };
    let start = chrono::NaiveDate::from_ymd_opt(2026, 8, 20)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis();

    telemetry_repo::insert_events(
        &database,
        &scope,
        &[event(start + 10_000, "late", "user-a")],
    )
    .await
    .unwrap();
    backfill_all(&database).await;
    assert_eq!(
        first_seen_repo::count_new_users_hybrid(
            &database,
            Some("app-order"),
            Some("prod"),
            start + 5_000,
            None,
        )
        .await
        .unwrap(),
        1
    );

    telemetry_repo::insert_events(
        &database,
        &scope,
        &[event(start + 1_000, "older-arrival", "user-a")],
    )
    .await
    .unwrap();
    refresh_and_clear_dirty(&database, "app-order", "prod").await;

    assert_eq!(
        first_seen_repo::count_new_users_hybrid(
            &database,
            Some("app-order"),
            Some("prod"),
            start + 5_000,
            None,
        )
        .await
        .unwrap(),
        0
    );
}

#[tokio::test]
async fn invalidated_first_seen_falls_back_to_raw_until_new_epoch_completes() {
    let database = database::connect("sqlite::memory:").await.unwrap();
    database::migrate(&database).await.unwrap();
    let scope = telemetry_repo::TelemetryScope {
        application_id: "app-rebuild".into(),
        environment_id: "prod".into(),
    };
    let start = chrono::NaiveDate::from_ymd_opt(2026, 8, 20)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis();

    telemetry_repo::insert_events(
        &database,
        &scope,
        &[
            event(start + 1_000, "before-a", "user-a"),
            event(start + 2_000, "before-b", "user-b"),
        ],
    )
    .await
    .unwrap();
    backfill_all(&database).await;

    first_seen_repo::invalidate(&database).await.unwrap();
    assert!(!first_seen_repo::backfill_complete(&database).await.unwrap());

    // While no complete epoch exists, the query must remain authoritative via the raw fallback.
    telemetry_repo::insert_events(
        &database,
        &scope,
        &[event(start + 3_000, "during-rebuild", "user-c")],
    )
    .await
    .unwrap();
    assert_eq!(
        first_seen_repo::count_new_users_hybrid(
            &database,
            Some("app-rebuild"),
            Some("prod"),
            start,
            None,
        )
        .await
        .unwrap(),
        3
    );

    backfill_all(&database).await;
    assert_eq!(
        first_seen_repo::count_new_users_hybrid(
            &database,
            Some("app-rebuild"),
            Some("prod"),
            start,
            None,
        )
        .await
        .unwrap(),
        3
    );
}
