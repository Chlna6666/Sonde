use sea_orm_migration::prelude::*;

use super::columns::{bigint, create_index, create_table};

pub(super) struct IngestNonceReplay;

impl MigrationName for IngestNonceReplay {
    fn name(&self) -> &str {
        "m20260831_000023_ingest_nonce_replay"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for IngestNonceReplay {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            manager,
            "ingest_nonce_replay",
            vec![
                ColumnDef::new(Alias::new("replay_key"))
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
            "idx_ingest_nonce_replay_expires",
            "ingest_nonce_replay",
            &["expires_at"],
            false,
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(Alias::new("ingest_nonce_replay"))
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
}
