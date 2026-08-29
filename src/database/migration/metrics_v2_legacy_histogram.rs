use sea_orm_migration::prelude::*;

pub(super) struct MetricsV2LegacyHistogramRepair;

impl MigrationName for MetricsV2LegacyHistogramRepair {
    fn name(&self) -> &str {
        "m20260829_000020_metrics_v2_legacy_histogram_repair"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for MetricsV2LegacyHistogramRepair {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if !manager
            .has_column("metric_points", "histogram_bucket_counts")
            .await?
        {
            return Ok(());
        }

        // Metrics v2 initially normalized a legacy `histogram + value` observation to count=1 with
        // no finite bounds but accidentally persisted `bucket_counts=[]`. Explicit histograms always
        // have bounds.len()+1 buckets, so the no-boundary form must contain one +Inf bucket `[1]`.
        manager
            .get_connection()
            .execute_unprepared(
                "UPDATE metric_points SET histogram_bucket_counts = '[1]' \
                 WHERE metric_type = 'histogram' \
                   AND histogram_count = 1 \
                   AND histogram_bounds = '[]' \
                   AND histogram_bucket_counts = '[]'",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Data repair is intentionally not reversed.
        Ok(())
    }
}
