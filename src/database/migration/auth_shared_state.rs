use sea_orm_migration::prelude::*;

use super::columns::{bigint, create_index, create_table, nullable_bigint, string};

pub(super) struct SharedAuthState;

impl MigrationName for SharedAuthState {
    fn name(&self) -> &str {
        "m20260828_000008_shared_auth_state"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for SharedAuthState {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            manager,
            "auth_sessions",
            vec![
                string("token_hash").primary_key().to_owned(),
                string("user_id"),
                string("csrf_token"),
                bigint("expires_at"),
                bigint("last_seen_at"),
                bigint("created_at"),
            ],
        )
        .await?;
        create_table(
            manager,
            "auth_2fa_pending",
            vec![
                string("token_hash").primary_key().to_owned(),
                string("user_id"),
                bigint("expires_at"),
                nullable_bigint("consumed_at"),
                bigint("created_at"),
            ],
        )
        .await?;
        create_table(
            manager,
            "auth_totp_replay",
            vec![
                string("user_id").primary_key().to_owned(),
                bigint("last_step"),
                bigint("updated_at"),
            ],
        )
        .await?;
        create_index(
            manager,
            "idx_auth_sessions_user",
            "auth_sessions",
            &["user_id"],
            false,
        )
        .await?;
        create_index(
            manager,
            "idx_auth_sessions_expires",
            "auth_sessions",
            &["expires_at"],
            false,
        )
        .await?;
        create_index(
            manager,
            "idx_auth_2fa_pending_expires",
            "auth_2fa_pending",
            &["expires_at"],
            false,
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in ["auth_totp_replay", "auth_2fa_pending", "auth_sessions"] {
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
