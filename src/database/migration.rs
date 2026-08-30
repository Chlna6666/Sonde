use sea_orm_migration::prelude::*;

mod alert_delivery_queue;
mod auth_shared_state;
mod columns;
mod daily_rollups;
mod device_profiles;
mod dimension_rollups;
mod dirty_source_mask;
mod error_model;
mod first_seen_epoch;
mod first_seen_index;
mod ingest_nonce_replay;
mod job_leases;
mod log_error_rollups;
mod metrics_v2;
mod metrics_v2_legacy_histogram;
mod rollup_generation;
mod tables;
mod user_rollup_chunks;
mod user_rollups;

use alert_delivery_queue::AlertDeliveryQueue;
use auth_shared_state::SharedAuthState;
use columns::{bigint, create_index, create_table, string};
use daily_rollups::DailyRollups;
use device_profiles::DeviceProfiles;
use dimension_rollups::DimensionRollups;
use dirty_source_mask::DirtySourceMask;
use error_model::ErrorTelemetryModel;
use first_seen_epoch::FirstSeenEpoch;
use first_seen_index::FirstSeenIndex;
use ingest_nonce_replay::IngestNonceReplay;
use job_leases::JobLeases;
use log_error_rollups::LogErrorRollups;
use metrics_v2::MetricsV2;
use metrics_v2_legacy_histogram::MetricsV2LegacyHistogramRepair;
use rollup_generation::RollupGeneration;
use tables::{
    create_alert_tables, create_application_tables, create_identity_tables, create_import_tables,
    create_telemetry_tables,
};
use user_rollup_chunks::UserRollupChunks;
use user_rollups::UserRollups;

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
            Box::new(OperationalIndexes),
            Box::new(ErrorTelemetryModel),
            Box::new(SharedAuthState),
            Box::new(DailyRollups),
            Box::new(RollupGeneration),
            Box::new(JobLeases),
            Box::new(DimensionRollups),
            Box::new(UserRollups),
            Box::new(UserRollupChunks),
            Box::new(FirstSeenIndex),
            Box::new(FirstSeenEpoch),
            Box::new(DirtySourceMask),
            Box::new(LogErrorRollups),
            Box::new(MetricsV2),
            Box::new(MetricsV2LegacyHistogramRepair),
            Box::new(AlertDeliveryQueue),
            Box::new(DeviceProfiles),
            Box::new(IngestNonceReplay),
        ]
    }
}

struct InitialSchema;
struct EphemeralSessions;
struct ApplicationMultiUserAndShowcase;
struct TwoFactorAuthAndChannels;
struct AlertsAndAuditTables;
struct OperationalIndexes;

impl MigrationName for AlertsAndAuditTables {
    fn name(&self) -> &str {
        "m20260828_000005_alerts_and_audit"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for AlertsAndAuditTables {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_alert_tables(manager).await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

impl MigrationName for OperationalIndexes {
    fn name(&self) -> &str {
        "m20260828_000006_operational_indexes"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for OperationalIndexes {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_index(
            manager,
            "idx_events_scope_name_time",
            "events",
            &["application_id", "environment_id", "name", "timestamp"],
            false,
        )
        .await?;
        create_index(
            manager,
            "idx_metrics_scope_name_time",
            "metric_points",
            &["application_id", "environment_id", "name", "timestamp"],
            false,
        )
        .await?;
        create_index(
            manager,
            "idx_logs_scope_level_time",
            "logs",
            &["application_id", "environment_id", "level", "timestamp"],
            false,
        )
        .await?;
        create_index(
            manager,
            "idx_role_bindings_user_app",
            "role_bindings",
            &["user_id", "application_id"],
            false,
        )
        .await?;
        create_index(
            manager,
            "idx_alert_rules_app_enabled",
            "alert_rules",
            &["application_id", "enabled"],
            false,
        )
        .await?;
        create_index(
            manager,
            "idx_alert_deliveries_created",
            "alert_deliveries",
            &["created_at"],
            false,
        )
        .await?;
        create_index(
            manager,
            "idx_audit_log_created",
            "audit_log",
            &["created_at"],
            false,
        )
        .await
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
        add_column_if_missing(
            manager,
            "users",
            "totp_secret",
            "ALTER TABLE users ADD COLUMN totp_secret TEXT",
        )
        .await?;
        add_column_if_missing(
            manager,
            "users",
            "totp_enabled",
            "ALTER TABLE users ADD COLUMN totp_enabled INTEGER NOT NULL DEFAULT 0",
        )
        .await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

impl MigrationName for InitialSchema {
    fn name(&self) -> &str {
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
        for (column, sql) in [
            (
                "owner_user_id",
                "ALTER TABLE applications ADD COLUMN owner_user_id TEXT",
            ),
            (
                "is_public",
                "ALTER TABLE applications ADD COLUMN is_public INTEGER NOT NULL DEFAULT 0",
            ),
            (
                "description",
                "ALTER TABLE applications ADD COLUMN description TEXT",
            ),
            (
                "github_url",
                "ALTER TABLE applications ADD COLUMN github_url TEXT",
            ),
            (
                "website_url",
                "ALTER TABLE applications ADD COLUMN website_url TEXT",
            ),
            (
                "custom_header",
                "ALTER TABLE applications ADD COLUMN custom_header TEXT",
            ),
        ] {
            add_column_if_missing(manager, "applications", column, sql).await?;
        }
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
        create_import_tables(manager).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in [
            "ingest_nonce_replay",
            "telemetry_devices",
            "telemetry_first_seen_backfill_days",
            "telemetry_user_first_seen",
            "telemetry_daily_user_sets",
            "telemetry_daily_dimensions",
            "telemetry_daily_log_errors",
            "job_leases",
            "telemetry_daily_rollups",
            "telemetry_dirty_days",
            "auth_totp_replay",
            "auth_2fa_pending",
            "auth_sessions",
            "error_occurrences",
            "error_groups",
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

async fn add_column_if_missing(
    manager: &SchemaManager<'_>,
    table: &str,
    column: &str,
    statement: &str,
) -> Result<(), DbErr> {
    if !manager.has_column(table, column).await? {
        manager
            .get_connection()
            .execute_unprepared(statement)
            .await?;
    }
    Ok(())
}
