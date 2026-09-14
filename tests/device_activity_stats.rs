#![allow(clippy::expect_used)]

use sonde::database::{
    self, activity_stats,
    device_activity::daily_activity,
    device_session,
    device_state::{DeviceObservation, DeviceTelemetryKind},
    telemetry::TelemetryScope,
};

const FIRST_SEEN: i64 = 1_788_307_180_000; // 2026-09-01T23:59:40Z
const SECOND_SEEN: i64 = 1_788_307_240_000; // 2026-09-02T00:00:40Z
const AFTER_IDLE: i64 = 1_788_310_800_000; // 2026-09-02T01:00:00Z
const AFTER_IDLE_ACTIVE: i64 = 1_788_310_860_000; // 2026-09-02T01:01:00Z

#[tokio::test]
async fn live_activity_is_split_across_utc_days_and_sessions_are_server_derived() {
    let database = database::connect("sqlite::memory:")
        .await
        .expect("in-memory SQLite should connect");
    database::migrate(&database)
        .await
        .expect("schema migration should complete");
    let (application_id, environment_id) = database::applications::create_application(
        &database,
        "Activity Test",
        "activity-test",
        None,
    )
    .await
    .expect("application should be created");
    let scope = TelemetryScope {
        application_id,
        environment_id,
    };
    let device_hash = "device-activity-test";

    observe(&database, &scope, device_hash, FIRST_SEEN).await;
    observe(&database, &scope, device_hash, SECOND_SEEN).await;
    observe(&database, &scope, device_hash, AFTER_IDLE).await;
    observe(&database, &scope, device_hash, AFTER_IDLE_ACTIVE).await;

    let days = daily_activity(
        &database,
        Some(&scope.application_id),
        Some(&scope.environment_id),
        None,
    )
    .await
    .expect("daily activity should be readable");
    assert_eq!(days.len(), 2);
    assert_eq!(days[0].day, "2026-09-01");
    assert_eq!(days[0].devices, 1);
    assert_eq!(days[0].active_millis, 20_000);
    assert_eq!(days[1].day, "2026-09-02");
    assert_eq!(days[1].devices, 1);
    assert_eq!(days[1].active_millis, 100_000);

    let sessions = device_session::summary(
        &database,
        Some(&scope.application_id),
        Some(&scope.environment_id),
        None,
        None,
    )
    .await
    .expect("session summary should be readable");
    assert_eq!(sessions.total_sessions, 2);
    assert_eq!(sessions.total_active_millis, 120_000);
    assert_eq!(sessions.average_session_millis, 60_000);

    let stats = activity_stats::query(
        &database,
        Some(&scope.application_id),
        Some(&scope.environment_id),
        None,
        Some(1),
    )
    .await
    .expect("activity statistics should be readable");
    assert_eq!(stats.summary.active_millis, 120_000);
    assert_eq!(stats.summary.lifetime_active_millis, 120_000);
    assert_eq!(stats.summary.sessions, 2);
    assert_eq!(stats.summary.lifetime_sessions, 2);
    assert_eq!(stats.summary.measured_devices, 1);
    assert_eq!(stats.summary.measurement_coverage_pct, 100.0);
    assert_eq!(stats.summary.average_session_millis, 60_000);
    assert_eq!(stats.summary.average_active_millis_per_device, 120_000);
    assert_eq!(
        stats
            .trend
            .last()
            .map(|point| point.cumulative_active_millis),
        Some(120_000)
    );
    assert_eq!(
        stats.trend.last().map(|point| point.cumulative_sessions),
        Some(2)
    );
    assert_eq!(
        stats
            .trend
            .last()
            .map(|point| point.lifetime_cumulative_active_millis),
        Some(120_000)
    );
    assert_eq!(
        stats
            .trend
            .last()
            .map(|point| point.lifetime_cumulative_sessions),
        Some(2)
    );

    let window = activity_stats::query(
        &database,
        Some(&scope.application_id),
        Some(&scope.environment_id),
        Some(AFTER_IDLE),
        Some(1),
    )
    .await
    .expect("windowed activity statistics should be readable");
    assert_eq!(window.summary.active_millis, 60_000);
    assert_eq!(window.summary.lifetime_active_millis, 120_000);
    assert_eq!(window.summary.sessions, 1);
    assert_eq!(window.summary.lifetime_sessions, 2);
    let last = window
        .trend
        .last()
        .expect("windowed activity trend should contain the second session");
    assert_eq!(last.cumulative_active_millis, 60_000);
    assert_eq!(last.cumulative_sessions, 1);
    assert_eq!(last.lifetime_cumulative_active_millis, 120_000);
    assert_eq!(last.lifetime_cumulative_sessions, 2);
}

async fn observe(
    database: &sea_orm::DatabaseConnection,
    scope: &TelemetryScope,
    device_hash: &str,
    received_at: i64,
) {
    sonde::database::device_state::observe(
        database,
        scope,
        device_hash,
        &DeviceObservation {
            kind: DeviceTelemetryKind::Heartbeat,
            received_at,
            telemetry_at: received_at,
            item_count: 0,
            session_id: None,
            app_version: None,
            launcher_version: None,
            os: None,
            system_language: None,
            architecture: None,
        },
    )
    .await
    .expect("device observation should succeed");
}
