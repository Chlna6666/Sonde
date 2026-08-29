#![allow(clippy::unwrap_used)]

use sea_orm::{ConnectionTrait, sea_query::{Alias, Expr, ExprTrait, Query}};
use sea_orm_migration::MigratorTrait;
use sonde::database::{self, app_repo, query::insert};

#[tokio::test]
async fn migration_20_repairs_legacy_single_observation_histogram_bucket() {
    let database = database::connect("sqlite::memory:").await.unwrap();

    // Apply through metrics v2 (migration 19), but deliberately stop before the repair migration.
    database::Migrator::up(&database, Some(19)).await.unwrap();
    let (application_id, environment_id) =
        app_repo::create_application(&database, "Migration Test", "migration-test", None)
            .await
            .unwrap();
    let timestamp = 1_777_680_000_000_i64;

    insert(
        &database,
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
            "legacy-bad-histogram".into(),
            application_id.into(),
            environment_id.into(),
            "legacy.duration".into(),
            "histogram".into(),
            12.5_f64.into(),
            "ms".into(),
            timestamp.into(),
            "{}".into(),
            timestamp.into(),
            1_i64.into(),
            12.5_f64.into(),
            12.5_f64.into(),
            12.5_f64.into(),
            "[]".into(),
            "[]".into(),
        ],
    )
    .await
    .unwrap();

    database::Migrator::up(&database, Some(1)).await.unwrap();

    let row = database
        .query_one(
            &Query::select()
                .column(Alias::new("histogram_bucket_counts"))
                .from(Alias::new("metric_points"))
                .and_where(Expr::col(Alias::new("id")).eq("legacy-bad-histogram"))
                .limit(1)
                .to_owned(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        row.try_get::<String>("", "histogram_bucket_counts")
            .unwrap(),
        "[1]"
    );
}
