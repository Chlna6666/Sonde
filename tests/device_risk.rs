#![allow(clippy::unwrap_used)]

use sonde::database::{
    self, device_risk,
    device_state::{self, DeviceObservation, DeviceTelemetryKind, TimedDimension},
    telemetry::TelemetryScope,
};

fn observation(received_at: i64, telemetry_at: i64, os: &str) -> DeviceObservation {
    DeviceObservation {
        kind: DeviceTelemetryKind::Event,
        received_at,
        telemetry_at,
        item_count: 1,
        session_id: None,
        app_version: Some(TimedDimension {
            value: "1.0.0".into(),
            timestamp: telemetry_at,
        }),
        launcher_version: None,
        os: Some(TimedDimension {
            value: os.into(),
            timestamp: telemetry_at,
        }),
        system_language: None,
        architecture: None,
    }
}

#[tokio::test]
async fn risk_is_read_by_device_scope_and_decays_only_after_quiet_cutoff()
-> Result<(), Box<dyn std::error::Error>> {
    let database = database::connect("sqlite::memory:").await?;
    database::migrate(&database).await?;
    let scope = TelemetryScope {
        application_id: "app-risk".into(),
        environment_id: "env-risk".into(),
    };
    let device_hash = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let now = chrono::Utc::now().timestamp_millis();

    device_state::observe(
        &database,
        &scope,
        device_hash,
        &observation(now, now, "windows"),
    )
    .await?;
    let baseline = device_risk::risk_score_for_device(
        &database,
        &scope.application_id,
        &scope.environment_id,
        device_hash,
    )
    .await?
    .unwrap();
    assert_eq!(baseline, 0);

    assert_eq!(
        device_risk::record_replay_detected(
            &database,
            &scope.application_id,
            &scope.environment_id,
            device_hash,
            now + 500,
        )
        .await?,
        1
    );
    let replay_risk = device_risk::risk_score_for_device(
        &database,
        &scope.application_id,
        &scope.environment_id,
        device_hash,
    )
    .await?
    .unwrap();
    assert_eq!(replay_risk, 2);

    device_state::observe(
        &database,
        &scope,
        device_hash,
        &observation(now + 1_000, now + 1_000, "linux"),
    )
    .await?;

    let risk = device_risk::risk_score_for_device(
        &database,
        &scope.application_id,
        &scope.environment_id,
        device_hash,
    )
    .await?
    .unwrap();
    assert!(risk > replay_risk);

    let untouched = device_risk::decay_scores(&database, now + 500).await?;
    assert_eq!(untouched, 0);
    let unchanged = device_risk::risk_score_for_device(
        &database,
        &scope.application_id,
        &scope.environment_id,
        device_hash,
    )
    .await?
    .unwrap();
    assert_eq!(unchanged, risk);

    let decayed = device_risk::decay_scores(&database, now + 2_000).await?;
    assert_eq!(decayed, 1);
    let after = device_risk::risk_score_for_device(
        &database,
        &scope.application_id,
        &scope.environment_id,
        device_hash,
    )
    .await?
    .unwrap();
    assert_eq!(after, risk - 1);
    Ok(())
}
