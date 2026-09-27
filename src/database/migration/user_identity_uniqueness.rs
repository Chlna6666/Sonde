use sea_orm_migration::prelude::*;

use super::columns::create_index;

pub(super) struct UserIdentityUniqueness;

impl MigrationName for UserIdentityUniqueness {
    fn name(&self) -> &str {
        "m20260927_000029_user_identity_uniqueness"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for UserIdentityUniqueness {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Authentication treats username as a single exact identifier. Enforce that invariant in
        // the database so concurrent user creation/update cannot produce an ambiguous account.
        create_index(manager, "uq_users_username", "users", &["username"], true).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("uq_users_username")
                    .table(Alias::new("users"))
                    .to_owned(),
            )
            .await
    }
}
