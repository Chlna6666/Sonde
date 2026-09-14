#![allow(clippy::unwrap_used)]

use sonde::{
    database::{log_error_rollup, rollups, telemetry, telemetry_count},
    domain::telemetry::{Attributes, LogInput, LogLevel, MetricInput, MetricType},
};

fn metric(timestamp: i64, value: f64) -> MetricInput {
    MetricInput {
        name: "cpu.usage".into(),
        metric_type: MetricType::Gauge,
        value: Some(value),
        histogram: None,
        unit: Some("percent".into()),
        timestamp: Some(timestamp),
        attributes: Attributes::new(),
    }
}

fn log(timestamp: i64, level: LogLevel, message: &str) -> LogInput {
    LogInput {
        level,
        message: message.into(),
        logger: Some("test".into()),
        trace_id: None,
        span_id: None,
        timestamp: Some(timestamp),
        attributes: Attributes::new(),
    }
}

async fn drain_daily_rollups(database: &sea_orm::DatabaseConnection) {
    loop {
        let dirty = rollups::list_dirty_days(database, 128, i64::MAX)
            .await
            .unwrap();
        if dirty.is_empty() {
            break;
        }
        for item in dirty {
            if item.has_source(log_error_rollup::DIRTY_SOURCE_LOG_ERROR) {
                assert!(
                    log_error_rollup::recompute_claimed_day(database, &item)
                        .await
                        .unwrap()
                );
            }
            assert!(
                rollups::recompute_claimed_day(database, item)
                    .await
                    .unwrap()
            );
        }
    }
}

#[tokio::test]
async fn scalar_counts_use_rollups_with_dirty_and_partial_fallbacks() {
    let database = sonde::database::connect("sqlite::memory:").await.unwrap();
    sonde::database::migrate(&database).await.unwrap();
    let scope = telemetry::TelemetryScope {
        application_id: "app-count".into(),
        environment_id: "prod".into(),
    };
    let day1 = chrono::NaiveDate::from_ymd_opt(2026, 8, 20)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis();
    let day2 = day1 + 86_400_000;

    telemetry::insert_metrics(
        &database,
        &scope,
        &[metric(day1 + 1_000, 1.0), metric(day2 + 1_000, 2.0)],
    )
    .await
    .unwrap();
    telemetry::insert_logs(
        &database,
        &scope,
        &[
            log(day1 + 2_000, LogLevel::Info, "one"),
            log(day1 + 3_000, LogLevel::Error, "error-one"),
            log(day2 + 2_000, LogLevel::Fatal, "fatal-two"),
        ],
    )
    .await
    .unwrap();

    rollups::seed_historical_dirty_days_once(&database)
        .await
        .unwrap();
    log_error_rollup::seed_historical_dirty_days_once(&database)
        .await
        .unwrap();
    drain_daily_rollups(&database).await;

    for (kind, expected) in [
        (telemetry_count::RollupCountKind::Metrics, 2),
        (telemetry_count::RollupCountKind::Logs, 3),
        (telemetry_count::RollupCountKind::ErrorLogs, 2),
    ] {
        assert_eq!(
            telemetry_count::count_hybrid(
                &database,
                kind,
                Some(&scope.application_id),
                Some(&scope.environment_id),
                Some(day1),
                Some(day2 + 86_400_000),
            )
            .await
            .unwrap(),
            expected
        );
    }

    telemetry::insert_metrics(&database, &scope, &[metric(day2 + 3_000, 3.0)])
        .await
        .unwrap();
    telemetry::insert_logs(
        &database,
        &scope,
        &[log(day2 + 4_000, LogLevel::Error, "error-three")],
    )
    .await
    .unwrap();
    assert_eq!(
        telemetry_count::count_hybrid(
            &database,
            telemetry_count::RollupCountKind::Metrics,
            Some(&scope.application_id),
            Some(&scope.environment_id),
            Some(day1),
            Some(day2 + 86_400_000),
        )
        .await
        .unwrap(),
        3
    );
    assert_eq!(
        telemetry_count::count_hybrid(
            &database,
            telemetry_count::RollupCountKind::ErrorLogs,
            Some(&scope.application_id),
            Some(&scope.environment_id),
            Some(day1),
            Some(day2 + 86_400_000),
        )
        .await
        .unwrap(),
        3
    );

    telemetry::insert_logs(
        &database,
        &scope,
        &[log(day2 + 5_000, LogLevel::Info, "informational")],
    )
    .await
    .unwrap();
    assert_eq!(
        telemetry_count::count_hybrid(
            &database,
            telemetry_count::RollupCountKind::ErrorLogs,
            Some(&scope.application_id),
            Some(&scope.environment_id),
            Some(day1),
            Some(day2 + 86_400_000),
        )
        .await
        .unwrap(),
        3
    );

    assert_eq!(
        telemetry_count::count_hybrid(
            &database,
            telemetry_count::RollupCountKind::Metrics,
            Some(&scope.application_id),
            Some(&scope.environment_id),
            Some(day1 + 1_500),
            Some(day2 + 86_400_000),
        )
        .await
        .unwrap(),
        2
    );
    assert_eq!(
        telemetry_count::count_hybrid(
            &database,
            telemetry_count::RollupCountKind::ErrorLogs,
            Some(&scope.application_id),
            Some(&scope.environment_id),
            Some(day1 + 3_500),
            Some(day2 + 86_400_000),
        )
        .await
        .unwrap(),
        2
    );
}
