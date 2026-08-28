use sea_orm_migration::prelude::*;

pub(super) struct DirtySourceMask;

impl MigrationName for DirtySourceMask {
    fn name(&self) -> &str {
        "m20260828_000017_dirty_source_mask"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for DirtySourceMask {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if !manager
            .has_column("telemetry_dirty_days", "source_mask")
            .await?
        {
            // Existing pending work predates source tracking. Treat it as touching every source so
            // the first post-upgrade recomputation is conservative and cannot leave stale fields.
            manager
                .get_connection()
                .execute_unprepared(
                    "ALTER TABLE telemetry_dirty_days ADD COLUMN source_mask BIGINT NOT NULL DEFAULT 15",
                )
                .await?;
        }
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Keep the additive column for portable SQLite downgrade semantics.
        Ok(())
    }
}
