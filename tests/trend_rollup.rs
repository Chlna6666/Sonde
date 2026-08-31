#![allow(clippy::unwrap_used)]

use sonde::{
    database::{self, rollups, telemetry, trends, user_rollup},
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

async fn build_user_and_daily_rollups(database: &sea_orm::DatabaseConnection) {
    rollups::seed_historical_dirty_days_once(database)
        .await
        .unwrap();
    user_rollup::seed_historical_user_dirty_days_once(database)
        .await
        .unwrap();

    loop {
        let dirty = rollups::list_dirty_days(database, 128, i64::MAX)
            .await
            .unwrap();
        if dirty.is_empty() {
            break;
        }
        for item in dirty {
            assert!(user_rollup::recompute_claimed_day_user_set(database, &item)
                .await
                .unwrap());
            assert!(rollups::recompute_claimed_day(database, item)
                .await
                .unwrap());
        }
    }
}

#[tokio::test]
async fn global_daily_users_are_deduplicated_across_applications() {
    let database = database::connect("sqlite::memory:").await.unwrap();
    database::migrate(&database).await.unwrap();
    let app_a = telemetry::TelemetryScope {
        application_id: "app-a".into(),
        environment_id: "prod".into(),
    };
    let app_b = telemetry::TelemetryScope {
        application_id: "app-b".into(),
        environment_id: "prod".into(),
    };
    let day = chrono::NaiveDate::from_ymd_opt(2026, 8, 20)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis();

    telemetry::insert_events(
        &database,
        &app_a,
        &[event(day + 1_000, "a-shared", "shared-user")],
    )
    .await
    .unwrap();
    telemetry::insert_events(
        &database,
        &app_b,
        &[event(day + 2_000, "b-shared", "shared-user")],
    )
    .await
    .unwrap();
    build_user_and_daily_rollups(&database).await;

    let points = trends::global_daily_hybrid(&database, Some(30), Some(day))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].day, "2026-08-20");
    assert_eq!(points[0].events, 2);
    assert_eq!(points[0].users, 1);
}

#[tokio::test]
async fn monthly_trend_unions_users_across_days_and_preserves_partial_start() {
    let database = database::connect("sqlite::memory:").await.unwrap();
    database::migrate(&database).await.unwrap();
    let app_a = telemetry::TelemetryScope {
        application_id: "app-a".into(),
        environment_id: "prod".into(),
    };
    let app_b = telemetry::TelemetryScope {
        application_id: "app-b".into(),
        environment_id: "prod".into(),
    };
    let august = chrono::NaiveDate::from_ymd_opt(2026, 8, 20)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis();
    let september = chrono::NaiveDate::from_ymd_opt(2026, 9, 5)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis();

    telemetry::insert_events(
        &database,
        &app_a,
        &[
            event(august + 1_000, "a-aug", "shared-user"),
            event(september + 1_000, "a-sep-shared", "shared-user"),
            event(september + 2_000, "a-sep-new", "new-user"),
        ],
    )
    .await
    .unwrap();
    telemetry::insert_events(
        &database,
        &app_b,
        &[event(august + 2_000, "b-aug", "shared-user")],
    )
    .await
    .unwrap();
    build_user_and_daily_rollups(&database).await;

    let global = trends::global_daily_hybrid(&database, Some(365), Some(august))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(global.len(), 2);
    assert_eq!((global[0].day.as_str(), global[0].events, global[0].users), ("2026-08", 2, 1));
    assert_eq!((global[1].day.as_str(), global[1].events, global[1].users), ("2026-09", 2, 2));

    let application = trends::application_daily_hybrid(
        &database,
        "app-a",
        None,
        Some(365),
        Some(august),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(application.len(), 2);
    assert_eq!((application[0].day.as_str(), application[0].events, application[0].users), ("2026-08", 1, 1));
    assert_eq!((application[1].day.as_str(), application[1].events, application[1].users), ("2026-09", 2, 2));

    // Starting between the two August events excludes app-a's first event but keeps app-b's event.
    // The monthly projection must replace that partial day from raw data rather than use the whole
    // cached day, while September remains rollup-backed.
    let partial = trends::global_daily_hybrid(&database, Some(365), Some(august + 1_500))
        .await
        .unwrap()
        .unwrap();
    assert_eq!((partial[0].day.as_str(), partial[0].events, partial[0].users), ("2026-08", 1, 1));
    assert_eq!((partial[1].day.as_str(), partial[1].events, partial[1].users), ("2026-09", 2, 2));
}
