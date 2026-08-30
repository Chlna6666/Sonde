use sea_orm_migration::prelude::*;

use super::columns::{bigint, create_index, create_table};

pub(super) struct IngestBootstrapLimits;

impl MigrationName for IngestBootstrapLimits {
    fn name(&self) -> &str {
        "m20260831_000024_ingest_bootstrap_limits"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for IngestBootstrapLimits {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            manager,
            "ingest_rate_windows",
            vec![
                ColumnDef::new(Alias::new("bucket_key"))
                    .string_len(64)
                    .primary_key()
                    .to_owned(),
                bigint("usage_count"),
                bigint("expires_at"),
                bigint("created_at"),
            ],
        )
        .await?;
        create_index(
            manager,
            "idx_ingest_rate_windows_expires",
            "ingest_rate_windows",
            &["expires_at"],
            false,
        )
        .await?;

        create_table(
            manager,
            "ingest_device_enrollments",
            vec![
                ColumnDef::new(Alias::new("enrollment_key"))
                    .string_len(64)
                    .primary_key()
                    .to_owned(),
                bigint("expires_at"),
                bigint("created_at"),
            ],
        )
        .await?;
        create_index(
            manager,
            "idx_ingest_device_enrollments_expires",
            "ingest_device_enrollments",
            &["expires_at"],
            false,
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in ["ingest_device_enrollments", "ingest_rate_windows"] {
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
