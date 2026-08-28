use sea_orm_migration::prelude::*;

use super::columns::{bigint, create_index, create_table, integer};

pub(super) struct UserRollupChunks;

impl MigrationName for UserRollupChunks {
    fn name(&self) -> &str {
        "m20260828_000014_user_rollup_chunks"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for UserRollupChunks {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // This cache first appeared in the immediately preceding migration and contains no
        // authoritative data. Recreate it with chunking so MySQL's standard BLOB size is never a
        // per-day unique-user ceiling. Invalidate readiness so readers use raw telemetry until the
        // background worker has rebuilt every historical day.
        drop_user_set_table(manager).await?;
        create_chunked_user_set_table(manager).await?;
        invalidate_readiness(manager).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // The cache is derived from events, so rollback can recreate the previous empty schema and
        // let the older worker backfill it instead of attempting a lossy chunk-to-row conversion.
        drop_user_set_table(manager).await?;
        create_legacy_user_set_table(manager).await?;
        invalidate_readiness(manager).await
    }
}

async fn create_chunked_user_set_table(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    create_table(
        manager,
        "telemetry_daily_user_sets",
        vec![
            ColumnDef::new(Alias::new("id"))
                .string_len(80)
                .not_null()
                .primary_key()
                .to_owned(),
            ColumnDef::new(Alias::new("application_id"))
                .string_len(64)
                .not_null()
                .to_owned(),
            ColumnDef::new(Alias::new("environment_id"))
                .string_len(64)
                .not_null()
                .to_owned(),
            ColumnDef::new(Alias::new("day"))
                .string_len(10)
                .not_null()
                .to_owned(),
            integer("chunk_index"),
            bigint("user_count"),
            ColumnDef::new(Alias::new("fingerprints"))
                .blob()
                .not_null()
                .to_owned(),
            bigint("updated_at"),
        ],
    )
    .await?;
    create_index(
        manager,
        "idx_telemetry_user_sets_scope_day_chunk",
        "telemetry_daily_user_sets",
        &["application_id", "environment_id", "day", "chunk_index"],
        false,
    )
    .await?;
    create_index(
        manager,
        "idx_telemetry_user_sets_env_day",
        "telemetry_daily_user_sets",
        &["environment_id", "day"],
        false,
    )
    .await
}

async fn create_legacy_user_set_table(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    create_table(
        manager,
        "telemetry_daily_user_sets",
        vec![
            ColumnDef::new(Alias::new("id"))
                .string_len(72)
                .not_null()
                .primary_key()
                .to_owned(),
            ColumnDef::new(Alias::new("application_id"))
                .string_len(64)
                .not_null()
                .to_owned(),
            ColumnDef::new(Alias::new("environment_id"))
                .string_len(64)
                .not_null()
                .to_owned(),
            ColumnDef::new(Alias::new("day"))
                .string_len(10)
                .not_null()
                .to_owned(),
            bigint("user_count"),
            ColumnDef::new(Alias::new("fingerprints"))
                .blob()
                .not_null()
                .to_owned(),
            bigint("updated_at"),
        ],
    )
    .await?;
    create_index(
        manager,
        "idx_telemetry_user_sets_scope_day",
        "telemetry_daily_user_sets",
        &["application_id", "environment_id", "day"],
        false,
    )
    .await?;
    create_index(
        manager,
        "idx_telemetry_user_sets_env_day",
        "telemetry_daily_user_sets",
        &["environment_id", "day"],
        false,
    )
    .await
}

async fn drop_user_set_table(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .drop_table(
            Table::drop()
                .table(Alias::new("telemetry_daily_user_sets"))
                .if_exists()
                .to_owned(),
        )
        .await
}

async fn invalidate_readiness(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .get_connection()
        .execute_unprepared(
            "DELETE FROM system_state WHERE key = 'telemetry_user_rollup_backfill_v1'",
        )
        .await?;
    Ok(())
}
