use sea_orm_migration::prelude::*;

use super::columns::{bigint, create_index, create_table};

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
                bounded_string("id", 72).primary_key().to_owned(),
                bounded_string("application_id", 64),
                bounded_string("environment_id", 64),
                bounded_string("day", 10),
                bigint("error_logs"),
                bigint("updated_at"),
            ],
        )
        .await?;
        // The deterministic primary key already enforces tuple uniqueness. Keep only the bounded
        // lookup index so utf8mb4 stays well below InnoDB's index-key budget.
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

fn bounded_string(name: &str, length: u32) -> ColumnDef {
    ColumnDef::new(Alias::new(name))
        .string_len(length)
        .not_null()
        .to_owned()
}
