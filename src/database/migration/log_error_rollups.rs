use sea_orm_migration::prelude::*;

use super::columns::{bigint, create_index, create_table, string};

pub(super) struct LogErrorRollups;

impl MigrationName for LogErrorRollups {
    fn name(&self) -> &str {
        "m20260829_000018_log_error_rollups"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for LogErrorRollups {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            manager,
            "telemetry_daily_log_errors",
            vec![
                string("id").primary_key().to_owned(),
                string("application_id"),
                string("environment_id"),
                string("day"),
                bigint("error_logs"),
                bigint("updated_at"),
            ],
        )
        .await?;
        create_index(
            manager,
            "uq_telemetry_log_errors_scope_day",
            "telemetry_daily_log_errors",
            &["application_id", "environment_id", "day"],
            true,
        )
        .await?;
        create_index(
            manager,
            "idx_telemetry_log_errors_scope_day",
            "telemetry_daily_log_errors",
            &["application_id", "environment_id", "day"],
            false,
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(Alias::new("telemetry_daily_log_errors"))
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
}
