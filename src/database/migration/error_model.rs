use sea_orm_migration::prelude::*;

use super::columns::{
    bigint, create_index, create_table, nullable_bigint, nullable_string, nullable_text, string,
    text,
};

pub(super) struct ErrorTelemetryModel;

impl MigrationName for ErrorTelemetryModel {
    fn name(&self) -> &str {
        "m20260828_000007_error_telemetry_model"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for ErrorTelemetryModel {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            manager,
            "error_groups",
            vec![
                string("id").primary_key().to_owned(),
                string("application_id"),
                string("environment_id"),
                string("fingerprint"),
                string("name"),
                text("message_sample"),
                string("severity"),
                bigint("first_seen"),
                bigint("last_seen"),
                bigint("occurrences"),
                nullable_string("last_app_version"),
                nullable_string("last_launcher_version"),
                nullable_string("last_os"),
                bigint("updated_at"),
            ],
        )
        .await?;
        create_table(
            manager,
            "error_occurrences",
            vec![
                string("id").primary_key().to_owned(),
                string("group_id"),
                string("application_id"),
                string("environment_id"),
                bigint("timestamp"),
                nullable_string("anonymous_id"),
                nullable_string("session_id"),
                nullable_string("app_version"),
                nullable_string("launcher_version"),
                nullable_string("os"),
                nullable_text("stack_trace"),
                nullable_bigint("handled"),
                text("attributes"),
                bigint("received_at"),
            ],
        )
        .await?;
        create_index(
            manager,
            "uq_error_groups_scope_fingerprint",
            "error_groups",
            &["application_id", "environment_id", "fingerprint"],
            true,
        )
        .await?;
        create_index(
            manager,
            "idx_error_groups_scope_last_seen",
            "error_groups",
            &["application_id", "environment_id", "last_seen"],
            false,
        )
        .await?;
        create_index(
            manager,
            "idx_error_occurrences_group_time",
            "error_occurrences",
            &["group_id", "timestamp"],
            false,
        )
        .await?;
        create_index(
            manager,
            "idx_error_occurrences_scope_time",
            "error_occurrences",
            &["application_id", "environment_id", "timestamp"],
            false,
        )
        .await?;
        create_index(
            manager,
            "idx_error_occurrences_scope_user_time",
            "error_occurrences",
            &[
                "application_id",
                "environment_id",
                "anonymous_id",
                "timestamp",
            ],
            false,
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in ["error_occurrences", "error_groups"] {
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
