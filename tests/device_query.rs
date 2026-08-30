#![allow(clippy::unwrap_used)]

use sonde::database::{
    self,
    device_query_repo::{self, DeviceProfileFilter},
    device_state_repo::{self, DeviceObservation, DeviceTelemetryKind, TimedDimension},
    telemetry_repo::TelemetryScope,
};

fn observation(received_at: i64, os: &str) -> DeviceObservation {
    DeviceObservation {
        kind: DeviceTelemetryKind::Event,
        received_at,
        telemetry_at: received_at,
        item_count: 1,
        session_id: Some(TimedDimension {
            value: "session".into(),
            timestamp: received_at,
        }),
        app_version: Some(TimedDimension {
            value: "1.0.0".into(),
            timestamp: received_at,
        }),
        launcher_version: None,
        os: Some(TimedDimension {
            value: os.into(),
            timestamp: received_at,
        }),
    }
}

#[tokio::test]
async fn query_repo_filters_device_risk_and_scope() -> Result<(), Box<dyn std::error::Error>> {
    let database = database::connect("sqlite::memory:").await?;
    database::migrate(&database).await?;
    let scope = TelemetryScope {
        application_id: "app-security-query".into(),
        environment_id: "env-production".into(),
    };
    let now = chrono::Utc::now().timestamp_millis();
    let low = "1111111111111111111111111111111111111111111111111111111111111111";
    let risky = "2222222222222222222222222222222222222222222222222222222222222222";

    device_state_repo::observe(&database, &scope, low, &observation(now, "windows")).await?;
    device_state_repo::observe(&database, &scope, risky, &observation(now, "windows")).await?;
    device_state_repo::observe(
        &database,
        &scope,
        risky,
        &observation(now + 1_000, "linux"),
    )
    .await?;

    let page = device_query_repo::list_profiles(
        &database,
        &DeviceProfileFilter {
            application_id: &scope.application_id,
            environment_id: Some(&scope.environment_id),
            last_seen_from: None,
            last_seen_to: None,
            min_risk: Some(20),
            max_risk: None,
            search: None,
            page: 1,
            page_size: 50,
        },
    )
    .await?;
    assert_eq!(page.total, 1);
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id, risky);
    assert!(page.items[0].risk_score >= 20);

    let summary = device_query_repo::security_summary(
        &database,
        &scope.application_id,
        Some(&scope.environment_id),
        now - 15 * 60_000,
        now - 24 * 60 * 60_000,
    )
    .await?;
    assert_eq!(summary.total, 2);
    assert_eq!(summary.active, 2);
    Ok(())
}
