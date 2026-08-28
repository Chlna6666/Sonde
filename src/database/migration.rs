use sea_orm_migration::prelude::*;

mod columns;
mod tables;

use columns::{bigint, create_table, string};
use tables::{
    create_application_tables, create_identity_tables, create_operations_tables,
    create_telemetry_tables,
};

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(InitialSchema),
            Box::new(EphemeralSessions),
            Box::new(ApplicationMultiUserAndShowcase),
            Box::new(TwoFactorAuthAndChannels),
            Box::new(AlertsAndAuditTables),
        ]
    }
}

struct InitialSchema;

struct EphemeralSessions;

struct ApplicationMultiUserAndShowcase;

struct TwoFactorAuthAndChannels;

struct AlertsAndAuditTables;

impl MigrationName for AlertsAndAuditTables {
    fn name(&self) -> &str {
        "m20260828_000005_alerts_and_audit"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for AlertsAndAuditTables {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_operations_tables(manager).await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

impl MigrationName for TwoFactorAuthAndChannels {
    fn name(&self) -> &str {
        "m20260828_000004_totp_2fa"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for TwoFactorAuthAndChannels {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        let _ = conn
            .execute_unprepared("ALTER TABLE users ADD COLUMN totp_secret TEXT")
            .await;
        let _ = conn
            .execute_unprepared("ALTER TABLE users ADD COLUMN totp_enabled INTEGER NOT NULL DEFAULT 0")
            .await;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

impl MigrationName for InitialSchema {
    fn name(&self) -> &str {
        // Keep the historical name so existing installations do not replay the base schema.
        "migration"
    }
}

impl MigrationName for EphemeralSessions {
    fn name(&self) -> &str {
        "m20260826_000002_ephemeral_sessions"
    }
}

impl MigrationName for ApplicationMultiUserAndShowcase {
    fn name(&self) -> &str {
        "m20260826_000003_application_multi_user_and_showcase"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for ApplicationMultiUserAndShowcase {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        let _ = conn
            .execute_unprepared("ALTER TABLE applications ADD COLUMN owner_user_id TEXT")
            .await;
        let _ = conn
            .execute_unprepared(
                "ALTER TABLE applications ADD COLUMN is_public INTEGER NOT NULL DEFAULT 0",
            )
            .await;
        let _ = conn
            .execute_unprepared("ALTER TABLE applications ADD COLUMN description TEXT")
            .await;
        let _ = conn
            .execute_unprepared("ALTER TABLE applications ADD COLUMN github_url TEXT")
            .await;
        let _ = conn
            .execute_unprepared("ALTER TABLE applications ADD COLUMN website_url TEXT")
            .await;
        let _ = conn
            .execute_unprepared("ALTER TABLE applications ADD COLUMN custom_header TEXT")
            .await;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

#[async_trait::async_trait]
impl MigrationTrait for EphemeralSessions {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(Alias::new("sessions"))
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        let rename = Query::update()
            .table(Alias::new("roles"))
            .value(Alias::new("name"), "Super Admin")
            .and_where(Expr::col(Alias::new("name")).eq("Owner"))
            .to_owned();
        manager.get_connection().execute(&rename).await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            manager,
            "sessions",
            vec![
                string("id").primary_key().to_owned(),
                string("user_id"),
                string("token_hash").unique_key().to_owned(),
                string("csrf_token"),
                bigint("expires_at"),
                bigint("created_at"),
            ],
        )
        .await?;
        let rename = Query::update()
            .table(Alias::new("roles"))
            .value(Alias::new("name"), "Owner")
            .and_where(Expr::col(Alias::new("name")).eq("Super Admin"))
            .to_owned();
        manager.get_connection().execute(&rename).await?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl MigrationTrait for InitialSchema {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_identity_tables(manager).await?;
        create_application_tables(manager).await?;
        create_telemetry_tables(manager).await?;
        create_operations_tables(manager).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in [
            "audit_log",
            "alert_deliveries",
            "notification_channels",
            "alert_rules",
            "import_runs",
            "daily_aggregates",
            "logs",
            "metric_points",
            "events",
            "api_keys",
            "environments",
            "role_bindings",
            "sessions",
            "roles",
            "users",
            "applications",
            "system_state",
        ] {
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
