use sea_orm_migration::prelude::*;

pub(super) struct CleanupLauncherVersion;

impl MigrationName for CleanupLauncherVersion {
    fn name(&self) -> &str {
        "m20261001_000030_cleanup_launcher_version"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for CleanupLauncherVersion {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_column_if_exists(manager, "telemetry_devices", "launcher_version_changes").await?;
        drop_column_if_exists(manager, "telemetry_devices", "last_launcher_version").await?;
        drop_column_if_exists(manager, "telemetry_devices", "last_launcher_version_at").await?;
        drop_column_if_exists(manager, "events", "launcher_version").await?;
        drop_column_if_exists(manager, "error_groups", "last_launcher_version").await?;
        drop_column_if_exists(manager, "error_occurrences", "launcher_version").await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

async fn drop_column_if_exists(
    manager: &SchemaManager<'_>,
    table: &str,
    column: &str,
) -> Result<(), DbErr> {
    if !manager.has_column(table, column).await? {
        return Ok(());
    }
    manager
        .alter_table(
            Table::alter()
                .table(Alias::new(table))
                .drop_column(Alias::new(column))
                .to_owned(),
        )
        .await
}
