use sea_orm_migration::prelude::*;

use super::columns::{bigint, create_index, create_table};

pub(super) struct DeviceSessions;

impl MigrationName for DeviceSessions {
    fn name(&self) -> &str {
        "m20260901_000028_device_sessions"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for DeviceSessions {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            manager,
            "telemetry_device_sessions",
            vec![
                bounded_string("id", 64).primary_key().to_owned(),
                bounded_string("application_id", 64),
                bounded_string("environment_id", 64),
                bounded_string("device_hash", 64),
                bigint("started_at"),
                bigint("last_seen_at"),
                bigint("active_millis"),
                bigint("request_count"),
                bigint("updated_at"),
            ],
        )
        .await?;
        create_index(
            manager,
            "idx_device_sessions_scope_started",
            "telemetry_device_sessions",
            &["application_id", "environment_id", "started_at"],
            false,
        )
        .await?;
        create_index(
            manager,
            "idx_device_sessions_device_started",
            "telemetry_device_sessions",
            &["application_id", "environment_id", "device_hash", "started_at"],
            false,
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(Alias::new("telemetry_device_sessions"))
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
