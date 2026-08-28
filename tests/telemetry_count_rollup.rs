#![allow(clippy::unwrap_used)]

use sonde::{
    database::{rollup_repo, telemetry_count_repo, telemetry_repo},
    domain::telemetry::{Attributes, LogInput, LogLevel, MetricInput, MetricType},
};

fn metric(timestamp: i64, value: f64) -> MetricInput {
    MetricInput {
        name: "cpu.usage".into(),
        metric_type: MetricType::Gauge,
        value,
        unit: Some("percent".into()),
        timestamp: Some(timestamp),
        attributes: Attributes::new(),
    }
}

fn log(timestamp: i64, message: &str) -> LogInput {
    LogInput {
        level: LogLevel::Info,
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
        let dirty = rollup_repo::list_dirty_days(database, 128, i64::MAX)
            .await
            .unwrap();
        if dirty.is_empty() {
            break;
        }
        for item in dirty {
            assert!(rollup_repo::recompute_claimed_day(database, item)
                .await
                .unwrap());
        }
    }
}

#[tokio::test]
async fn metric_and_log_counts_use_rollups_with_dirty_and_partial_fallbacks() {
    let database = sonde::database::connect("sqlite::memory:").await.unwrap();
    sonde::database::migrate(&database).await.unwrap();
    let scope = telemetry_repo::TelemetryScope {
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

    telemetry_repo::insert_metrics(
        &database,
        &scope,
        &[metric(day1 + 1_000, 1.0), metric(day2 + 1_000, 2.0)],
    )
    .await
    .unwrap();
    telemetry_repo::insert_logs(
        &database,
        &scope,
        &[log(day1 + 2_000, "one"), log(day2 + 2_000, "two")],
    )
    .await
    .unwrap();

    rollup_repo::seed_historical_dirty_days_once(&database)
        .await
        .unwrap();
    drain_daily_rollups(&database).await;

    assert_eq!(
        telemetry_count_repo::count_hybrid(
            &database,
            telemetry_count_repo::RollupCountKind::Metrics,
            Some(&scope.application_id),
            Some(&scope.environment_id),
            Some(day1),
            Some(day2 + 86_400_000),
        )
        .await
        .unwrap(),
        2
    );
    assert_eq!(
        telemetry_count_repo::count_hybrid(
            &database,
            telemetry_count_repo::RollupCountKind::Logs,
            Some(&scope.application_id),
            Some(&scope.environment_id),
            Some(day1),
            Some(day2 + 86_400_000),
        )
        .await
        .unwrap(),
        2
    );

    // A new metric is visible immediately even before its dirty day is folded back into the rollup.
    telemetry_repo::insert_metrics(&database, &scope, &[metric(day2 + 3_000, 3.0)])
        .await
        .unwrap();
    assert_eq!(
        telemetry_count_repo::count_hybrid(
            &database,
            telemetry_count_repo::RollupCountKind::Metrics,
            Some(&scope.application_id),
            Some(&scope.environment_id),
            Some(day1),
            Some(day2 + 86_400_000),
        )
        .await
        .unwrap(),
        3
    );

    // Exact millisecond boundaries replace only the partial first day from raw telemetry.
    assert_eq!(
        telemetry_count_repo::count_hybrid(
            &database,
            telemetry_count_repo::RollupCountKind::Metrics,
            Some(&scope.application_id),
            Some(&scope.environment_id),
            Some(day1 + 1_500),
            Some(day2 + 86_400_000),
        )
        .await
        .unwrap(),
        2
    );
}
