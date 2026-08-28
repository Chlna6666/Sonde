use sea_orm_migration::prelude::*;

use super::columns::{bigint, create_index, create_table, string};

pub(super) struct DimensionRollups;

impl MigrationName for DimensionRollups {
    fn name(&self) -> &str {
        "m20260828_000012_dimension_rollups"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for DimensionRollups {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            manager,
            "telemetry_daily_dimensions",
            vec![
                string("id").primary_key().to_owned(),
                string("application_id"),
                string("environment_id"),
                string("day"),
                string("dimension"),
                string("dimension_value"),
                bigint("count"),
                bigint("updated_at"),
            ],
        )
        .await?;
        // The deterministic SHA-256 primary key already enforces uniqueness for the full
        // (app, env, day, dimension, value) tuple. Avoid a redundant five-VARCHAR unique index:
        // with utf8mb4 it can exceed InnoDB's 3072-byte index-key budget.
        create_index(
            manager,
            "idx_telemetry_dim_scope_dimension_day",
            "telemetry_daily_dimensions",
            &["application_id", "environment_id", "dimension", "day"],
            false,
        )
        .await?;
        create_index(
            manager,
            "idx_telemetry_dim_env_dimension_day",
            "telemetry_daily_dimensions",
            &["environment_id", "dimension", "day"],
            false,
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(Alias::new("telemetry_daily_dimensions"))
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
}
