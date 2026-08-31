#![allow(clippy::unwrap_used)]

use sonde::{
    database::{self, dimension_rollup, rollups, telemetry},
    domain::telemetry::{Attributes, EventInput},
};

fn event(
    timestamp: i64,
    idempotency_key: &str,
    app_version: &str,
    launcher_version: &str,
    os: &str,
) -> EventInput {
    EventInput {
        name: "application.start".into(),
        timestamp: Some(timestamp),
        anonymous_id: Some(format!("device-{idempotency_key}")),
        session_id: Some(format!("session-{idempotency_key}")),
        app_version: Some(app_version.into()),
        launcher_version: Some(launcher_version.into()),
        os: Some(os.into()),
        idempotency_key: Some(idempotency_key.into()),
        attributes: Attributes::new(),
    }
}

async fn recompute_environment_day(
    database: &sea_orm::DatabaseConnection,
    application_id: &str,
    environment_id: &str,
) {
    let dirty = rollups::list_dirty_days(database, 32, i64::MAX)
        .await
        .unwrap()
        .into_iter()
        .find(|item| {
            item.application_id == application_id && item.environment_id == environment_id
        })
        .unwrap();
    assert!(
        dimension_rollup::recompute_claimed_day_dimensions(database, &dirty)
            .await
            .unwrap()
    );
    assert!(rollups::recompute_claimed_day(database, dirty)
        .await
        .unwrap());
}

#[tokio::test]
async fn dimension_rollup_uses_clean_rollups_dirty_raw_and_exact_boundary() {
    let database = database::connect("sqlite::memory:").await.unwrap();
    database::migrate(&database).await.unwrap();
    let scope = telemetry::TelemetryScope {
        application_id: "app-a".into(),
        environment_id: "prod".into(),
    };
    let start = chrono::NaiveDate::from_ymd_opt(2026, 8, 20)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis();

    telemetry::insert_events(
        &database,
        &scope,
        &[
            event(
                start + 3_600_000,
                "evt-1",
                "1.0.0",
                "10.0.0",
                "Windows 11 Build 26100",
            ),
            event(
                start + 13 * 3_600_000,
                "evt-2",
                "2.0.0",
                "10.1.0",
                "Ubuntu 24.04 Linux x86_64",
            ),
            event(
                start + 14 * 3_600_000,
                "evt-3",
                "2.0.0",
                "10.1.0",
                "Ubuntu 24.04 Linux x86_64",
            ),
        ],
    )
    .await
    .unwrap();

    assert_eq!(
        dimension_rollup::seed_historical_dimension_dirty_days_once(&database)
            .await
            .unwrap(),
        1
    );
    recompute_environment_day(&database, "app-a", "prod").await;

    let clean = dimension_rollup::event_dimension_timeline_hybrid(
        &database,
        Some("app-a"),
        Some("prod"),
        Some(start),
        dimension_rollup::DIMENSION_APP_VERSION,
    )
    .await
    .unwrap()
    .unwrap();
    let clean_counts = dimension_rollup::aggregate_dimension(&clean);
    assert_eq!(clean_counts[0].value, "2.0.0");
    assert_eq!(clean_counts[0].count, 2);
    assert_eq!(clean_counts[1].value, "1.0.0");
    assert_eq!(clean_counts[1].count, 1);

    let os = dimension_rollup::event_dimension_timeline_hybrid(
        &database,
        Some("app-a"),
        Some("prod"),
        Some(start),
        dimension_rollup::DIMENSION_OS,
    )
    .await
    .unwrap()
    .unwrap();
    let families = dimension_rollup::aggregate_os_families(&os);
    assert_eq!(families[0].value, "Linux");
    assert_eq!(families[0].count, 2);
    assert_eq!(families[1].value, "Windows");
    assert_eq!(families[1].count, 1);

    telemetry::insert_events(
        &database,
        &scope,
        &[event(
            start + 15 * 3_600_000,
            "evt-4",
            "3.0.0",
            "10.2.0",
            "Windows 11 Build 26100",
        )],
    )
    .await
    .unwrap();

    let dirty = dimension_rollup::event_dimension_timeline_hybrid(
        &database,
        Some("app-a"),
        Some("prod"),
        Some(start),
        dimension_rollup::DIMENSION_APP_VERSION,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        dimension_rollup::aggregate_dimension(&dirty)
            .iter()
            .map(|item| item.count)
            .sum::<u64>(),
        4
    );

    let partial = dimension_rollup::event_dimension_timeline_hybrid(
        &database,
        Some("app-a"),
        Some("prod"),
        Some(start + 12 * 3_600_000),
        dimension_rollup::DIMENSION_APP_VERSION,
    )
    .await
    .unwrap()
    .unwrap();
    let partial_counts = dimension_rollup::aggregate_dimension(&partial);
    assert_eq!(partial_counts.iter().map(|item| item.count).sum::<u64>(), 3);
    assert!(!partial_counts.iter().any(|item| item.value == "1.0.0"));
    assert_eq!(
        partial_counts
            .iter()
            .find(|item| item.value == "2.0.0")
            .unwrap()
            .count,
        2
    );
    assert_eq!(
        partial_counts
            .iter()
            .find(|item| item.value == "3.0.0")
            .unwrap()
            .count,
        1
    );
}
