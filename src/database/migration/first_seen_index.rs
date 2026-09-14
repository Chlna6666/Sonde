use sea_orm_migration::prelude::*;

use super::columns::{bigint, create_index, create_table};

pub(super) struct FirstSeenIndex;

impl MigrationName for FirstSeenIndex {
    fn name(&self) -> &str {
        "m20260828_000015_first_seen_index"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for FirstSeenIndex {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            manager,
            "telemetry_user_first_seen",
            vec![
                ColumnDef::new(Alias::new("id"))
                    .string_len(80)
                    .not_null()
                    .primary_key()
                    .to_owned(),
                ColumnDef::new(Alias::new("scope_kind"))
                    .string_len(8)
                    .not_null()
                    .to_owned(),
                ColumnDef::new(Alias::new("application_id"))
                    .string_len(64)
                    .not_null()
                    .to_owned(),
                ColumnDef::new(Alias::new("environment_id"))
                    .string_len(64)
                    .not_null()
                    .to_owned(),
                bigint("first_seen_at"),
                ColumnDef::new(Alias::new("first_seen_day"))
                    .string_len(10)
                    .not_null()
                    .to_owned(),
                bigint("updated_at"),
            ],
        )
        .await?;
        create_index(
            manager,
            "idx_user_first_seen_scope_time",
            "telemetry_user_first_seen",
            &[
                "scope_kind",
                "application_id",
                "environment_id",
                "first_seen_at",
            ],
            false,
        )
        .await?;

        create_table(
            manager,
            "telemetry_first_seen_backfill_days",
            vec![
                ColumnDef::new(Alias::new("id"))
                    .string_len(80)
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
                bigint("created_at"),
            ],
        )
        .await?;
        create_index(
            manager,
            "idx_first_seen_backfill_day",
            "telemetry_first_seen_backfill_days",
            &["day", "application_id", "environment_id"],
            false,
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in [
            "telemetry_first_seen_backfill_days",
            "telemetry_user_first_seen",
        ] {
            manager
                .drop_table(
                    Table::drop()
                        .table(Alias::new(table))
                        .if_exists()
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }
}
