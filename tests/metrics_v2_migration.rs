#![allow(clippy::unwrap_used)]

use sea_orm::{
    ConnectionTrait,
    sea_query::{Alias, Expr, ExprTrait, Query},
};
use sea_orm_migration::MigratorTrait;
use sonde::database::{self, app_repo, query::insert};

#[tokio::test]
async fn migration_20_repairs_all_no_boundary_histogram_buckets() {
    let database = database::connect("sqlite::memory:").await.unwrap();

    // Apply through metrics v2 (migration 19), but deliberately stop before the repair migration.
    database::Migrator::up(&database, Some(19)).await.unwrap();
    let (application_id, environment_id) =
        app_repo::create_application(&database, "Migration Test", "migration-test", None)
            .await
            .unwrap();
    let timestamp = 1_777_680_000_000_i64;

    insert_broken_histogram(
        &database,
        &application_id,
        &environment_id,
        "legacy-bad-histogram",
        1,
        12.5,
        timestamp,
    )
    .await;
    insert_broken_histogram(
        &database,
        &application_id,
        &environment_id,
        "aggregate-bad-histogram",
        4,
        10.0,
        timestamp + 1,
    )
    .await;

    database::Migrator::up(&database, Some(1)).await.unwrap();

    assert_eq!(bucket_counts(&database, "legacy-bad-histogram").await, "[1]");
    assert_eq!(
        bucket_counts(&database, "aggregate-bad-histogram").await,
        "[4]"
    );
}

async fn insert_broken_histogram(
    database: &sea_orm::DatabaseConnection,
    application_id: &str,
    environment_id: &str,
    id: &str,
    count: i64,
    value: f64,
    timestamp: i64,
) {
    insert(
        database,
        "metric_points",
        &[
            "id",
            "application_id",
            "environment_id",
            "name",
            "metric_type",
            "value",
            "unit",
            "timestamp",
            "attributes",
            "received_at",
            "histogram_count",
            "histogram_sum",
            "histogram_min",
            "histogram_max",
            "histogram_bounds",
            "histogram_bucket_counts",
        ],
        vec![
            id.into(),
            application_id.into(),
            environment_id.into(),
            "legacy.duration".into(),
            "histogram".into(),
            value.into(),
            "ms".into(),
            timestamp.into(),
            "{}".into(),
            timestamp.into(),
            count.into(),
            (value * count as f64).into(),
            value.into(),
            value.into(),
            "[]".into(),
            "[]".into(),
        ],
    )
    .await
    .unwrap();
}

async fn bucket_counts(database: &sea_orm::DatabaseConnection, id: &str) -> String {
    database
        .query_one(
            &Query::select()
                .column(Alias::new("histogram_bucket_counts"))
                .from(Alias::new("metric_points"))
                .and_where(Expr::col(Alias::new("id")).eq(id))
                .limit(1)
                .to_owned(),
        )
        .await
        .unwrap()
        .unwrap()
        .try_get("", "histogram_bucket_counts")
        .unwrap()
}
