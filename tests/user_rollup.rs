#![allow(clippy::unwrap_used)]

use sonde::{
    database::{self, rollup_repo, telemetry_repo, user_rollup_repo},
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

async fn recompute_scope_day(
    database: &sea_orm::DatabaseConnection,
    application_id: &str,
    environment_id: &str,
) {
    let dirty = rollup_repo::list_dirty_days(database, 32, i64::MAX)
        .await
        .unwrap()
        .into_iter()
        .find(|item| {
            item.application_id == application_id && item.environment_id == environment_id
        })
        .unwrap();
    assert!(
        user_rollup_repo::recompute_claimed_day_user_set(database, &dirty)
            .await
            .unwrap()
    );
    assert!(rollup_repo::recompute_claimed_day(database, dirty)
        .await
        .unwrap());
}

#[tokio::test]
async fn user_rollup_merges_clean_days_dirty_days_and_partial_boundaries() {
    let database = database::connect("sqlite::memory:").await.unwrap();
    database::migrate(&database).await.unwrap();
    let scope = telemetry_repo::TelemetryScope {
        application_id: "app-a".into(),
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
            event(start + 3_600_000, "evt-1", "user-a"),
            event(start + 2 * 3_600_000, "evt-2", "user-a"),
            event(start + 13 * 3_600_000, "evt-3", "user-b"),
        ],
    )
    .await
    .unwrap();

    assert_eq!(
        user_rollup_repo::seed_historical_user_dirty_days_once(&database)
            .await
            .unwrap(),
        1
    );
    recompute_scope_day(&database, "app-a", "prod").await;

    assert_eq!(
        user_rollup_repo::unique_users_hybrid(
            &database,
            Some("app-a"),
            Some("prod"),
            Some(start),
            Some(start + 86_400_000),
        )
        .await
        .unwrap(),
        2
    );

    telemetry_repo::insert_events(
        &database,
        &scope,
        &[event(start + 15 * 3_600_000, "evt-4", "user-c")],
    )
    .await
    .unwrap();

    // The stored clean set is now stale, but the dirty marker forces an authoritative raw read.
    assert_eq!(
        user_rollup_repo::unique_users_hybrid(
            &database,
            Some("app-a"),
            Some("prod"),
            Some(start),
            Some(start + 86_400_000),
        )
        .await
        .unwrap(),
        3
    );

    // The first boundary day is only partially included; user-a occurred before noon and must not
    // leak in from the full-day cached set.
    assert_eq!(
        user_rollup_repo::unique_users_hybrid(
            &database,
            Some("app-a"),
            Some("prod"),
            Some(start + 12 * 3_600_000),
            Some(start + 86_400_000),
        )
        .await
        .unwrap(),
        2
    );

    let next_day = start + 86_400_000;
    telemetry_repo::insert_events(
        &database,
        &scope,
        &[
            event(next_day + 3_600_000, "evt-5", "user-b"),
            event(next_day + 2 * 3_600_000, "evt-6", "user-d"),
        ],
    )
    .await
    .unwrap();

    // Cross-day union must deduplicate user-b rather than summing daily unique counts.
    assert_eq!(
        user_rollup_repo::unique_users_hybrid(
            &database,
            Some("app-a"),
            Some("prod"),
            Some(start),
            Some(next_day + 86_400_000),
        )
        .await
        .unwrap(),
        4
    );
}
