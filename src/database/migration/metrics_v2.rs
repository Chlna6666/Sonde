use sea_orm_migration::prelude::*;

pub(super) struct MetricsV2;

impl MigrationName for MetricsV2 {
    fn name(&self) -> &str {
        "m20260829_000019_metrics_v2"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for MetricsV2 {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        add_column_if_missing(
            manager,
            "histogram_count",
            ColumnDef::new(Alias::new("histogram_count"))
                .big_integer()
                .null()
                .to_owned(),
        )
        .await?;
        add_column_if_missing(
            manager,
            "histogram_sum",
            ColumnDef::new(Alias::new("histogram_sum"))
                .double()
                .null()
                .to_owned(),
        )
        .await?;
        add_column_if_missing(
            manager,
            "histogram_min",
            ColumnDef::new(Alias::new("histogram_min"))
                .double()
                .null()
                .to_owned(),
        )
        .await?;
        add_column_if_missing(
            manager,
            "histogram_max",
            ColumnDef::new(Alias::new("histogram_max"))
                .double()
                .null()
                .to_owned(),
        )
        .await?;
        add_column_if_missing(
            manager,
            "histogram_bounds",
            ColumnDef::new(Alias::new("histogram_bounds"))
                .text()
                .null()
                .to_owned(),
        )
        .await?;
        add_column_if_missing(
            manager,
            "histogram_bucket_counts",
            ColumnDef::new(Alias::new("histogram_bucket_counts"))
                .text()
                .null()
                .to_owned(),
        )
        .await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Additive fields are intentionally retained for portable SQLite downgrade semantics.
        Ok(())
    }
}

async fn add_column_if_missing(
    manager: &SchemaManager<'_>,
    name: &str,
    column: ColumnDef,
) -> Result<(), DbErr> {
    if manager.has_column("metric_points", name).await? {
        return Ok(());
    }
    manager
        .alter_table(
            Table::alter()
                .table(Alias::new("metric_points"))
                .add_column(&mut column.to_owned())
                .to_owned(),
        )
        .await
}
