use sea_orm_migration::prelude::*;

pub(super) struct DeviceFacts;

impl MigrationName for DeviceFacts {
    fn name(&self) -> &str {
        "m20260901_000025_device_facts"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for DeviceFacts {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        add_column_if_missing(
            manager,
            "last_system_language",
            ColumnDef::new(Alias::new("last_system_language"))
                .string_len(64)
                .null()
                .to_owned(),
        )
        .await?;
        add_column_if_missing(
            manager,
            "last_system_language_at",
            ColumnDef::new(Alias::new("last_system_language_at"))
                .big_integer()
                .null()
                .to_owned(),
        )
        .await?;
        add_column_if_missing(
            manager,
            "last_architecture",
            ColumnDef::new(Alias::new("last_architecture"))
                .string_len(64)
                .null()
                .to_owned(),
        )
        .await?;
        add_column_if_missing(
            manager,
            "last_architecture_at",
            ColumnDef::new(Alias::new("last_architecture_at"))
                .big_integer()
                .null()
                .to_owned(),
        )
        .await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Additive fields are retained for portable SQLite downgrade semantics.
        Ok(())
    }
}

async fn add_column_if_missing(
    manager: &SchemaManager<'_>,
    name: &str,
    column: ColumnDef,
) -> Result<(), DbErr> {
    if manager.has_column("telemetry_devices", name).await? {
        return Ok(());
    }
    manager
        .alter_table(
            Table::alter()
                .table(Alias::new("telemetry_devices"))
                .add_column(column)
                .to_owned(),
        )
        .await
}
