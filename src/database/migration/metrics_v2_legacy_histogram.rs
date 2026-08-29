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

        // Metrics v2 initially allowed no-boundary histograms to persist `bucket_counts=[]` for any
        // population size. Explicit histograms always have bounds.len()+1 buckets, so each affected
        // row needs one +Inf bucket whose count equals the complete population.
        let select = Query::select()
            .columns([Alias::new("id"), Alias::new("histogram_count")])
            .from(Alias::new("metric_points"))
            .and_where(Expr::col(Alias::new("metric_type")).eq("histogram"))
            .and_where(Expr::col(Alias::new("histogram_count")).is_not_null())
            .and_where(Expr::col(Alias::new("histogram_bounds")).eq("[]"))
            .and_where(Expr::col(Alias::new("histogram_bucket_counts")).eq("[]"))
            .to_owned();
        let rows = manager.get_connection().query_all(&select).await?;
        for row in rows {
            let id: String = row.try_get("", "id")?;
            let count: i64 = row.try_get("", "histogram_count")?;
            if count < 0 {
                return Err(DbErr::Custom(
                    "negative histogram count found during metrics v2 repair".into(),
                ));
            }
            let update = Query::update()
                .table(Alias::new("metric_points"))
                .value(
                    Alias::new("histogram_bucket_counts"),
                    format!("[{count}]"),
                )
                .and_where(Expr::col(Alias::new("id")).eq(id))
                .to_owned();
            manager.get_connection().execute(&update).await?;
        }
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Data repair is intentionally not reversed.
        Ok(())
    }
}
