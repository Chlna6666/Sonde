use sea_orm_migration::prelude::*;

use super::columns::{bigint, create_index, create_table, string};

pub(super) struct JobLeases;

impl MigrationName for JobLeases {
    fn name(&self) -> &str {
        "m20260828_000011_job_leases"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for JobLeases {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            manager,
            "job_leases",
            vec![
                string("name").primary_key().to_owned(),
                string("holder_id"),
                bigint("lease_until"),
                bigint("updated_at"),
            ],
        )
        .await?;
        create_index(
            manager,
            "idx_job_leases_expiry",
            "job_leases",
            &["lease_until"],
            false,
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(Alias::new("job_leases"))
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
}
