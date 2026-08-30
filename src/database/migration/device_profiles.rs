use sea_orm_migration::prelude::*;

use super::columns::{bigint, create_index, create_table};

pub(super) struct DeviceProfiles;

impl MigrationName for DeviceProfiles {
    fn name(&self) -> &str {
        "m20260830_000022_device_profiles"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for DeviceProfiles {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            manager,
            "telemetry_devices",
            vec![
                bounded_string("id", 64).primary_key().to_owned(),
                bounded_string("application_id", 64),
                bounded_string("environment_id", 64),
                bounded_string("device_hash", 64),
                bigint("last_seen_at"),
                nullable_bigint("last_event_at"),
                nullable_bigint("last_metric_at"),
                nullable_bigint("last_log_at"),
                nullable_bigint("last_error_at"),
                nullable_bounded_string("last_session_id", 256),
                nullable_bigint("last_session_at"),
                nullable_bounded_string("last_app_version", 128),
                nullable_bigint("last_app_version_at"),
                nullable_bounded_string("last_launcher_version", 128),
                nullable_bigint("last_launcher_version_at"),
                nullable_bounded_string("last_os", 256),
                nullable_bigint("last_os_at"),
                bigint("event_items"),
                bigint("metric_items"),
                bigint("log_items"),
                bigint("error_items"),
                bigint("session_changes"),
                bigint("app_version_changes"),
                bigint("launcher_version_changes"),
                bigint("os_changes"),
                ColumnDef::new(Alias::new("risk_score"))
                    .integer()
                    .not_null()
                    .to_owned(),
                ColumnDef::new(Alias::new("last_anomaly"))
                    .string_len(256)
                    .null()
                    .to_owned(),
                nullable_bigint("last_anomaly_at"),
                bigint("updated_at"),
            ],
        )
        .await?;
        create_index(
            manager,
            "idx_telemetry_devices_scope_seen",
            "telemetry_devices",
            &["application_id", "environment_id", "last_seen_at"],
            false,
        )
        .await?;
        create_index(
            manager,
            "idx_telemetry_devices_app_risk",
            "telemetry_devices",
            &["application_id", "risk_score", "last_seen_at"],
            false,
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(Alias::new("telemetry_devices"))
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

fn nullable_bounded_string(name: &str, length: u32) -> ColumnDef {
    ColumnDef::new(Alias::new(name))
        .string_len(length)
        .null()
        .to_owned()
}

fn nullable_bigint(name: &str) -> ColumnDef {
    ColumnDef::new(Alias::new(name))
        .big_integer()
        .null()
        .to_owned()
}
