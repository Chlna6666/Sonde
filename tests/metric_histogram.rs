#![allow(clippy::unwrap_used)]

use sea_orm::{
    ConnectionTrait,
    sea_query::{Alias, Expr, ExprTrait, Query},
};
use sonde::{
    database::{self, explorer, telemetry},
    domain::telemetry::{Attributes, HistogramInput, MetricInput, MetricType, ValidateTelemetry},
};

fn aggregate_histogram(timestamp: i64) -> MetricInput {
    MetricInput {
        name: "http.request.duration".into(),
        metric_type: MetricType::Histogram,
        value: None,
        histogram: Some(HistogramInput {
            count: 6,
            sum: Some(63.0),
            min: Some(1.0),
            max: Some(25.0),
            explicit_bounds: vec![5.0, 10.0, 20.0],
            bucket_counts: vec![1, 2, 2, 1],
        }),
        unit: Some("ms".into()),
        timestamp: Some(timestamp),
        attributes: Attributes::new(),
    }
}

fn legacy_histogram(timestamp: i64) -> MetricInput {
    MetricInput {
        name: "legacy.duration".into(),
        metric_type: MetricType::Histogram,
        value: Some(12.5),
        histogram: None,
        unit: Some("ms".into()),
        timestamp: Some(timestamp),
        attributes: Attributes::new(),
    }
}

#[test]
fn metric_json_keeps_legacy_scalar_shape_and_accepts_aggregate_histograms() {
    let legacy: MetricInput = serde_json::from_value(serde_json::json!({
        "name": "cpu.usage",
        "metricType": "gauge",
        "value": 42.5,
        "unit": "percent",
        "attributes": {}
    }))
    .unwrap();
    assert_eq!(legacy.value, Some(42.5));
    assert!(legacy.histogram.is_none());
    assert!(legacy.validate().is_ok());

    let aggregate: MetricInput = serde_json::from_value(serde_json::json!({
        "name": "http.request.duration",
        "metricType": "histogram",
        "histogram": {
            "count": 4,
            "sum": 30.0,
            "min": 2.0,
            "max": 15.0,
            "explicitBounds": [5.0, 10.0],
            "bucketCounts": [1, 2, 1]
        },
        "unit": "ms",
        "attributes": {}
    }))
    .unwrap();
    assert!(aggregate.value.is_none());
    assert!(aggregate.validate().is_ok());
    assert_eq!(aggregate.histogram.unwrap().bucket_counts, vec![1, 2, 1]);
}

#[tokio::test]
async fn histogram_population_survives_storage_and_explorer_projection() {
    let database = database::connect("sqlite::memory:").await.unwrap();
    database::migrate(&database).await.unwrap();
    let scope = telemetry::TelemetryScope {
        application_id: "app-histogram".into(),
        environment_id: "prod".into(),
    };
    let timestamp = chrono::NaiveDate::from_ymd_opt(2026, 8, 29)
        .unwrap()
        .and_hms_opt(1, 2, 3)
        .unwrap()
        .and_utc()
        .timestamp_millis();

    telemetry::insert_metrics(
        &database,
        &scope,
        &[aggregate_histogram(timestamp), legacy_histogram(timestamp + 1)],
    )
    .await
    .unwrap();

    let aggregate = database
        .query_one(
            &Query::select()
                .columns(
                    [
                        "value",
                        "histogram_count",
                        "histogram_sum",
                        "histogram_min",
                        "histogram_max",
                        "histogram_bounds",
                        "histogram_bucket_counts",
                    ]
                    .map(Alias::new),
                )
                .from(Alias::new("metric_points"))
                .and_where(Expr::col(Alias::new("name")).eq("http.request.duration"))
                .limit(1)
                .to_owned(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(aggregate.try_get::<i64>("", "histogram_count").unwrap(), 6);
    assert_eq!(aggregate.try_get::<f64>("", "histogram_sum").unwrap(), 63.0);
    assert_eq!(aggregate.try_get::<f64>("", "histogram_min").unwrap(), 1.0);
    assert_eq!(aggregate.try_get::<f64>("", "histogram_max").unwrap(), 25.0);
    assert_eq!(aggregate.try_get::<f64>("", "value").unwrap(), 10.5);
    assert_eq!(
        serde_json::from_str::<Vec<f64>>(
            &aggregate
                .try_get::<String>("", "histogram_bounds")
                .unwrap()
        )
        .unwrap(),
        vec![5.0, 10.0, 20.0]
    );
    assert_eq!(
        serde_json::from_str::<Vec<u64>>(
            &aggregate
                .try_get::<String>("", "histogram_bucket_counts")
                .unwrap()
        )
        .unwrap(),
        vec![1, 2, 2, 1]
    );

    let legacy = database
        .query_one(
            &Query::select()
                .columns(
                    [
                        "histogram_count",
                        "histogram_sum",
                        "histogram_min",
                        "histogram_max",
                        "histogram_bounds",
                        "histogram_bucket_counts",
                    ]
                    .map(Alias::new),
                )
                .from(Alias::new("metric_points"))
                .and_where(Expr::col(Alias::new("name")).eq("legacy.duration"))
                .limit(1)
                .to_owned(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(legacy.try_get::<i64>("", "histogram_count").unwrap(), 1);
    assert_eq!(legacy.try_get::<f64>("", "histogram_sum").unwrap(), 12.5);
    assert_eq!(legacy.try_get::<f64>("", "histogram_min").unwrap(), 12.5);
    assert_eq!(legacy.try_get::<f64>("", "histogram_max").unwrap(), 12.5);
    assert_eq!(
        serde_json::from_str::<Vec<f64>>(
            &legacy.try_get::<String>("", "histogram_bounds").unwrap()
        )
        .unwrap(),
        Vec::<f64>::new()
    );
    assert_eq!(
        serde_json::from_str::<Vec<u64>>(
            &legacy
                .try_get::<String>("", "histogram_bucket_counts")
                .unwrap()
        )
        .unwrap(),
        vec![1]
    );

    let aggregate_page = explorer::metrics(
        &database,
        &explorer::ExplorerFilter {
            application_id: scope.application_id.clone(),
            environment_id: Some(scope.environment_id.clone()),
            from: None,
            to: None,
            name: Some("http.request.duration".into()),
            level: None,
            text: None,
            page: 1,
            page_size: 10,
        },
    )
    .await
    .unwrap();
    assert_eq!(aggregate_page.items.len(), 1);
    let histogram = aggregate_page.items[0].histogram.as_ref().unwrap();
    assert_eq!(histogram.count, 6);
    assert_eq!(histogram.sum, Some(63.0));
    assert_eq!(histogram.explicit_bounds, vec![5.0, 10.0, 20.0]);
    assert_eq!(histogram.bucket_counts, vec![1, 2, 2, 1]);

    let legacy_page = explorer::metrics(
        &database,
        &explorer::ExplorerFilter {
            application_id: scope.application_id,
            environment_id: Some(scope.environment_id),
            from: None,
            to: None,
            name: Some("legacy.duration".into()),
            level: None,
            text: None,
            page: 1,
            page_size: 10,
        },
    )
    .await
    .unwrap();
    let legacy_histogram = legacy_page.items[0].histogram.as_ref().unwrap();
    assert_eq!(legacy_histogram.count, 1);
    assert_eq!(legacy_histogram.explicit_bounds, Vec::<f64>::new());
    assert_eq!(legacy_histogram.bucket_counts, vec![1]);
}
