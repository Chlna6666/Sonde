use sea_orm_migration::prelude::*;

use super::columns::{bigint, create_index, create_table, string};

pub(super) struct DailyRollups;

impl MigrationName for DailyRollups {
    fn name(&self) -> &str {
        "m20260828_000009_daily_rollups"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for DailyRollups {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            manager,
            "telemetry_dirty_days",
            vec![
                string("id").primary_key().to_owned(),
                string("application_id"),
                string("environment_id"),
                string("day"),
                bigint("marked_at"),
            ],
        )
        .await?;
        create_table(
            manager,
            "telemetry_daily_rollups",
            vec![
                string("id").primary_key().to_owned(),
                string("application_id"),
                string("environment_id"),
                string("day"),
                bigint("events"),
                bigint("users"),
                bigint("metrics"),
                bigint("logs"),
                bigint("errors"),
                bigint("updated_at"),
            ],
        )
        .await?;
        create_index(
            manager,
            "uq_telemetry_dirty_scope_day",
            "telemetry_dirty_days",
            &["application_id", "environment_id", "day"],
            true,
        )
        .await?;
        create_index(
            manager,
            "idx_telemetry_dirty_marked",
            "telemetry_dirty_days",
            &["marked_at"],
            false,
        )
        .await?;
        create_index(
            manager,
            "uq_telemetry_rollup_scope_day",
            "telemetry_daily_rollups",
            &["application_id", "environment_id", "day"],
            true,
        )
        .await?;
        create_index(
            manager,
            "idx_telemetry_rollup_scope_day",
            "telemetry_daily_rollups",
            &["application_id", "environment_id", "day"],
            false,
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in ["telemetry_daily_rollups", "telemetry_dirty_days"] {
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
