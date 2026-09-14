#![allow(clippy::unwrap_used)]

use sea_orm::{
    ConnectionTrait,
    sea_query::{Alias, Expr, ExprTrait, Query},
};
use sonde::{
    database::{self, dimension_rollup, rollups, telemetry, user_rollup},
    domain::telemetry::{Attributes, EventInput, LogInput, LogLevel, MetricInput, MetricType},
};

fn event(timestamp: i64) -> EventInput {
    EventInput {
        name: "application.start".into(),
        timestamp: Some(timestamp),
        anonymous_id: Some("user-a".into()),
        session_id: Some("session-a".into()),
        app_version: Some("1.0.0".into()),
        launcher_version: Some("1.0.0".into()),
        os: Some("test".into()),
        idempotency_key: Some("event-a".into()),
        attributes: Attributes::new(),
        ..Default::default()
    }
}

fn metric(timestamp: i64) -> MetricInput {
    MetricInput {
        name: "cpu.usage".into(),
        metric_type: MetricType::Gauge,
        value: Some(42.0),
        histogram: None,
        unit: Some("percent".into()),
        timestamp: Some(timestamp),
        attributes: Attributes::new(),
    }
}

fn log(timestamp: i64) -> LogInput {
    LogInput {
        level: LogLevel::Info,
        message: "ready".into(),
        logger: Some("test".into()),
        trace_id: None,
        span_id: None,
        timestamp: Some(timestamp),
        attributes: Attributes::new(),
    }
}

async fn environment_dirty(
    database: &sea_orm::DatabaseConnection,
    application_id: &str,
    environment_id: &str,
) -> rollups::DirtyDay {
    rollups::list_dirty_days(database, 16, i64::MAX)
        .await
        .unwrap()
        .into_iter()
        .find(|dirty| {
            dirty.application_id == application_id && dirty.environment_id == environment_id
        })
        .unwrap()
}

#[tokio::test]
async fn source_mask_merges_without_invalidating_unrelated_rollup_fields() {
    let database = database::connect("sqlite::memory:").await.unwrap();
    database::migrate(&database).await.unwrap();
    let scope = telemetry::TelemetryScope {
        application_id: "app-mask".into(),
        environment_id: "prod".into(),
    };
    let day_start = chrono::NaiveDate::from_ymd_opt(2026, 8, 20)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis();

    telemetry::insert_events(&database, &scope, &[event(day_start + 1_000)])
        .await
        .unwrap();
    let event_dirty =
        environment_dirty(&database, &scope.application_id, &scope.environment_id).await;
    assert_eq!(event_dirty.source_mask, rollups::DIRTY_SOURCE_EVENT);
    assert!(
        rollups::recompute_claimed_day(&database, event_dirty)
            .await
            .unwrap()
    );

    telemetry::insert_metrics(&database, &scope, &[metric(day_start + 2_000)])
        .await
        .unwrap();
    telemetry::insert_logs(&database, &scope, &[log(day_start + 3_000)])
        .await
        .unwrap();

    let non_event_dirty =
        environment_dirty(&database, &scope.application_id, &scope.environment_id).await;
    assert_eq!(
        non_event_dirty.source_mask,
        rollups::DIRTY_SOURCE_METRIC | rollups::DIRTY_SOURCE_LOG
    );
    assert!(!non_event_dirty.has_source(rollups::DIRTY_SOURCE_EVENT));
    assert!(non_event_dirty.has_source(rollups::DIRTY_SOURCE_METRIC));
    assert!(non_event_dirty.has_source(rollups::DIRTY_SOURCE_LOG));

    assert!(
        rollups::recompute_claimed_day(&database, non_event_dirty)
            .await
            .unwrap()
    );

    let row = database
        .query_one(
            &Query::select()
                .columns(["events", "users", "metrics", "logs", "errors"].map(Alias::new))
                .from(Alias::new("telemetry_daily_rollups"))
                .and_where(Expr::col(Alias::new("application_id")).eq(&scope.application_id))
                .and_where(Expr::col(Alias::new("environment_id")).eq(&scope.environment_id))
                .limit(1)
                .to_owned(),
        )
        .await
        .unwrap()
        .unwrap();

    assert_eq!(row.try_get::<i64>("", "events").unwrap(), 1);
    assert_eq!(row.try_get::<i64>("", "users").unwrap(), 1);
    assert_eq!(row.try_get::<i64>("", "metrics").unwrap(), 1);
    assert_eq!(row.try_get::<i64>("", "logs").unwrap(), 1);
    assert_eq!(row.try_get::<i64>("", "errors").unwrap(), 0);
}

#[tokio::test]
async fn historical_event_seeds_or_event_into_existing_metric_dirty_marker() {
    let database = database::connect("sqlite::memory:").await.unwrap();
    database::migrate(&database).await.unwrap();
    let scope = telemetry::TelemetryScope {
        application_id: "app-seed-mask".into(),
        environment_id: "prod".into(),
    };
    let day_start = chrono::NaiveDate::from_ymd_opt(2026, 8, 21)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis();

    telemetry::insert_events(&database, &scope, &[event(day_start + 1_000)])
        .await
        .unwrap();
    let initial = environment_dirty(&database, &scope.application_id, &scope.environment_id).await;
    assert!(
        rollups::recompute_claimed_day(&database, initial)
            .await
            .unwrap()
    );

    telemetry::insert_metrics(&database, &scope, &[metric(day_start + 2_000)])
        .await
        .unwrap();
    let metric_only =
        environment_dirty(&database, &scope.application_id, &scope.environment_id).await;
    assert_eq!(metric_only.source_mask, rollups::DIRTY_SOURCE_METRIC);

    assert_eq!(
        dimension_rollup::seed_historical_dimension_dirty_days_once(&database)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        user_rollup::seed_historical_user_dirty_days_once(&database)
            .await
            .unwrap(),
        1
    );

    let promoted = environment_dirty(&database, &scope.application_id, &scope.environment_id).await;
    assert!(promoted.has_source(rollups::DIRTY_SOURCE_EVENT));
    assert!(promoted.has_source(rollups::DIRTY_SOURCE_METRIC));
    assert_eq!(
        promoted.source_mask,
        rollups::DIRTY_SOURCE_EVENT | rollups::DIRTY_SOURCE_METRIC
    );
}
