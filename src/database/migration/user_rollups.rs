use sea_orm_migration::prelude::*;

use super::columns::{bigint, create_index, create_table};

pub(super) struct UserRollups;

impl MigrationName for UserRollups {
    fn name(&self) -> &str {
        "m20260828_000013_user_rollups"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for UserRollups {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            manager,
            "telemetry_daily_user_sets",
            vec![
                ColumnDef::new(Alias::new("id"))
                    .string_len(72)
                    .not_null()
                    .primary_key()
                    .to_owned(),
                ColumnDef::new(Alias::new("application_id"))
                    .string_len(64)
                    .not_null()
                    .to_owned(),
                ColumnDef::new(Alias::new("environment_id"))
                    .string_len(64)
                    .not_null()
                    .to_owned(),
                ColumnDef::new(Alias::new("day"))
                    .string_len(10)
                    .not_null()
                    .to_owned(),
                bigint("user_count"),
                ColumnDef::new(Alias::new("fingerprints"))
                    .blob()
                    .not_null()
                    .to_owned(),
                bigint("updated_at"),
            ],
        )
        .await?;
        create_index(
            manager,
            "idx_telemetry_user_sets_scope_day",
            "telemetry_daily_user_sets",
            &["application_id", "environment_id", "day"],
            false,
        )
        .await?;
        create_index(
            manager,
            "idx_telemetry_user_sets_env_day",
            "telemetry_daily_user_sets",
            &["environment_id", "day"],
            false,
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(Alias::new("telemetry_daily_user_sets"))
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
}
