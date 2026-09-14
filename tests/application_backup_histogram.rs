#![allow(clippy::unwrap_used)]

use sea_orm::{
    ConnectionTrait,
    sea_query::{Alias, Expr, ExprTrait, Query},
};
use sonde::{
    database::{self, application_backup, applications, telemetry},
    domain::telemetry::{Attributes, HistogramInput, MetricInput, MetricType},
};

fn histogram_metric(timestamp: i64) -> MetricInput {
    MetricInput {
        name: "backup.histogram".into(),
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

#[tokio::test]
async fn single_application_backup_preserves_histogram_population() {
    let source = database::connect("sqlite::memory:").await.unwrap();
    database::migrate(&source).await.unwrap();
    let (application_id, environment_id) =
        applications::create_application(&source, "Histogram App", "histogram-app", None)
            .await
            .unwrap();
    let timestamp = 1_777_680_000_000_i64;
    telemetry::insert_metrics(
        &source,
        &telemetry::TelemetryScope {
            application_id: application_id.clone(),
            environment_id,
        },
        &[histogram_metric(timestamp)],
    )
    .await
    .unwrap();

    let backup = application_backup::export_single_application(&source, &application_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(backup.format_version, application_backup::FORMAT_VERSION);
    assert_eq!(backup.telemetry.metric_points.len(), 1);
    let histogram = backup.telemetry.metric_points[0]
        .histogram
        .as_ref()
        .unwrap();
    assert_eq!(histogram.count, 6);
    assert_eq!(histogram.explicit_bounds, vec![5.0, 10.0, 20.0]);
    assert_eq!(histogram.bucket_counts, vec![1, 2, 2, 1]);

    let target = database::connect("sqlite::memory:").await.unwrap();
    database::migrate(&target).await.unwrap();
    let imported_application = application_backup::import_single_application(&target, None, backup)
        .await
        .unwrap();
    assert_histogram_row(&target, &imported_application).await;
}

#[test]
fn scalar_metric_without_histogram_is_accepted() {
    let metric: application_backup::ExportedMetricPoint =
        serde_json::from_value(serde_json::json!({
            "environmentId": "prod",
            "name": "cpu.usage",
            "metricType": "gauge",
            "value": 0.5,
            "unit": "%",
            "timestamp": 1777680000000_i64,
            "attributes": {},
            "receivedAt": 1777680000000_i64
        }))
        .unwrap();
    assert!(metric.histogram.is_none());
    assert_eq!(metric.value, 0.5);
}

async fn assert_histogram_row(database: &sea_orm::DatabaseConnection, application_id: &str) {
    let row = database
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
                .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
                .and_where(Expr::col(Alias::new("name")).eq("backup.histogram"))
                .limit(1)
                .to_owned(),
        )
        .await
        .unwrap()
        .unwrap();

    assert_eq!(row.try_get::<i64>("", "histogram_count").unwrap(), 6);
    assert_eq!(row.try_get::<f64>("", "histogram_sum").unwrap(), 63.0);
    assert_eq!(row.try_get::<f64>("", "histogram_min").unwrap(), 1.0);
    assert_eq!(row.try_get::<f64>("", "histogram_max").unwrap(), 25.0);
    assert_eq!(
        serde_json::from_str::<Vec<f64>>(&row.try_get::<String>("", "histogram_bounds").unwrap())
            .unwrap(),
        vec![5.0, 10.0, 20.0]
    );
    assert_eq!(
        serde_json::from_str::<Vec<u64>>(
            &row.try_get::<String>("", "histogram_bucket_counts")
                .unwrap()
        )
        .unwrap(),
        vec![1, 2, 2, 1]
    );
}
