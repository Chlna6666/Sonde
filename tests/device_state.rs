#![allow(clippy::unwrap_used)]

use sea_orm::{
    ConnectionTrait,
    sea_query::{Alias, Expr, ExprTrait, Query},
};
use sonde::database::{self, device_state, telemetry::TelemetryScope};

fn observation(
    kind: device_state::DeviceTelemetryKind,
    received_at: i64,
    telemetry_at: i64,
    app_version: Option<&str>,
    os: Option<&str>,
) -> device_state::DeviceObservation {
    device_state::DeviceObservation {
        kind,
        received_at,
        telemetry_at,
        item_count: 1,
        session_id: None,
        app_version: app_version.map(|value| device_state::TimedDimension {
            value: value.into(),
            timestamp: telemetry_at,
        }),
        launcher_version: None,
        os: os.map(|value| device_state::TimedDimension {
            value: value.into(),
            timestamp: telemetry_at,
        }),
        system_language: None,
        architecture: None,
    }
}

#[tokio::test]
async fn device_profile_is_server_owned_and_monotonic() -> Result<(), Box<dyn std::error::Error>> {
    let database = database::connect("sqlite::memory:").await?;
    database::migrate(&database).await?;
    let scope = TelemetryScope {
        application_id: "app-device-state".into(),
        environment_id: "env-device-state".into(),
    };
    let device_hash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let now = chrono::Utc::now().timestamp_millis();

    device_state::observe(
        &database,
        &scope,
        device_hash,
        &observation(
            device_state::DeviceTelemetryKind::Event,
            now,
            now,
            Some("2.0.0"),
            Some("windows"),
        ),
    )
    .await?;
    device_state::observe(
        &database,
        &scope,
        device_hash,
        &observation(
            device_state::DeviceTelemetryKind::Metric,
            now + 500,
            now + 500,
            None,
            None,
        ),
    )
    .await?;
    device_state::observe(
        &database,
        &scope,
        device_hash,
        &observation(
            device_state::DeviceTelemetryKind::Event,
            now + 1_000,
            now - 60_000,
            Some("1.0.0"),
            Some("linux"),
        ),
    )
    .await?;

    let row = database
        .query_one(
            &Query::select()
                .columns(
                    [
                        "last_app_version",
                        "last_os",
                        "metric_items",
                        "app_version_changes",
                        "os_changes",
                    ]
                    .map(Alias::new),
                )
                .from(Alias::new("telemetry_devices"))
                .and_where(Expr::col(Alias::new("id")).eq(device_hash))
                .limit(1)
                .to_owned(),
        )
        .await?
        .unwrap();
    assert_eq!(row.try_get::<String>("", "last_app_version")?, "2.0.0");
    assert_eq!(row.try_get::<String>("", "last_os")?, "windows");
    assert_eq!(row.try_get::<i64>("", "metric_items")?, 1);
    assert_eq!(row.try_get::<i64>("", "app_version_changes")?, 0);
    assert_eq!(row.try_get::<i64>("", "os_changes")?, 0);

    device_state::observe(
        &database,
        &scope,
        device_hash,
        &observation(
            device_state::DeviceTelemetryKind::Error,
            now + 2_000,
            now + 2_000,
            Some("2.0.0"),
            Some("linux"),
        ),
    )
    .await?;
    let row = database
        .query_one(
            &Query::select()
                .columns(["last_os", "os_changes", "risk_score", "last_anomaly"].map(Alias::new))
                .from(Alias::new("telemetry_devices"))
                .and_where(Expr::col(Alias::new("id")).eq(device_hash))
                .limit(1)
                .to_owned(),
        )
        .await?
        .unwrap();
    assert_eq!(row.try_get::<String>("", "last_os")?, "linux");
    assert_eq!(row.try_get::<i64>("", "os_changes")?, 1);
    assert!(row.try_get::<i32>("", "risk_score")? > 0);
    assert!(
        row.try_get::<Option<String>>("", "last_anomaly")?
            .is_some_and(|value| value.contains("os_changed"))
    );
    Ok(())
}
