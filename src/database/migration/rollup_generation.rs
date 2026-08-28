use sea_orm_migration::prelude::*;

pub(super) struct RollupGeneration;

impl MigrationName for RollupGeneration {
    fn name(&self) -> &str {
        "m20260828_000010_rollup_generation"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for RollupGeneration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if !manager
            .has_column("telemetry_dirty_days", "generation")
            .await?
        {
            manager
                .get_connection()
                .execute_unprepared(
                    "ALTER TABLE telemetry_dirty_days ADD COLUMN generation BIGINT NOT NULL DEFAULT 1",
                )
                .await?;
        }
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // SQLite cannot portably drop a column on all supported versions. Keep the additive column.
        Ok(())
    }
}
