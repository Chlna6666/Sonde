use sea_orm::{ConnectionTrait, QueryResult};
use sea_orm_migration::prelude::*;

use crate::database::query::insert_batch_ignore_conflicts;

use super::columns::{bigint, create_index, create_table};

pub(super) struct DeviceActivityDays;

impl MigrationName for DeviceActivityDays {
    fn name(&self) -> &str {
        "m20260901_000026_device_activity_days"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for DeviceActivityDays {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        add_first_seen_if_missing(manager).await?;
        create_table(
            manager,
            "telemetry_device_activity_days",
            vec![
                bounded_string("id", 64).primary_key().to_owned(),
                bounded_string("application_id", 64),
                bounded_string("environment_id", 64),
                bounded_string("device_hash", 64),
                bounded_string("day", 10),
                bigint("first_seen_at"),
                bigint("last_seen_at"),
                bigint("active_millis"),
                bigint("request_count"),
                bigint("updated_at"),
            ],
        )
        .await?;
        create_index(
            manager,
            "idx_device_activity_scope_day",
            "telemetry_device_activity_days",
            &["application_id", "environment_id", "day"],
            false,
        )
        .await?;
        create_index(
            manager,
            "idx_device_activity_device_day",
            "telemetry_device_activity_days",
            &["application_id", "environment_id", "device_hash", "day"],
            true,
        )
        .await?;
        backfill_existing_profiles(manager).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(Alias::new("telemetry_device_activity_days"))
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
}

async fn add_first_seen_if_missing(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    if !manager
        .has_column("telemetry_devices", "first_seen_at")
        .await?
    {
        manager
            .alter_table(
                Table::alter()
                    .table(Alias::new("telemetry_devices"))
                    .add_column(
                        ColumnDef::new(Alias::new("first_seen_at"))
                            .big_integer()
                            .null()
                            .to_owned(),
                    )
                    .to_owned(),
            )
            .await?;
    }
    let backfill = Query::update()
        .table(Alias::new("telemetry_devices"))
        .value(
            Alias::new("first_seen_at"),
            Expr::col(Alias::new("last_seen_at")),
        )
        .and_where(Expr::col(Alias::new("first_seen_at")).is_null())
        .to_owned();
    manager.get_connection().execute(&backfill).await?;
    Ok(())
}

async fn backfill_existing_profiles(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    let query = Query::select()
        .columns(
            [
                "application_id",
                "environment_id",
                "device_hash",
                "last_seen_at",
            ]
            .map(Alias::new),
        )
        .from(Alias::new("telemetry_devices"))
        .to_owned();
    let rows = manager.get_connection().query_all(&query).await?;
    for chunk in rows.chunks(100) {
        backfill_chunk(manager.get_connection(), chunk).await?;
    }
    Ok(())
}

async fn backfill_chunk(
    database: &impl ConnectionTrait,
    rows: &[QueryResult],
) -> Result<(), DbErr> {
    let mut values = Vec::with_capacity(rows.len());
    for row in rows {
        let application_id: String = row.try_get("", "application_id")?;
        let environment_id: String = row.try_get("", "environment_id")?;
        let device_hash: String = row.try_get("", "device_hash")?;
        let last_seen_at: i64 = row.try_get("", "last_seen_at")?;
        let day = day_for_timestamp(last_seen_at);
        values.push(vec![
            activity_id(&application_id, &environment_id, &device_hash, &day).into(),
            application_id.into(),
            environment_id.into(),
            device_hash.into(),
            day.into(),
            last_seen_at.into(),
            last_seen_at.into(),
            0_i64.into(),
            1_i64.into(),
            last_seen_at.into(),
        ]);
    }
    insert_batch_ignore_conflicts(
        database,
        "telemetry_device_activity_days",
        &[
            "id",
            "application_id",
            "environment_id",
            "device_hash",
            "day",
            "first_seen_at",
            "last_seen_at",
            "active_millis",
            "request_count",
            "updated_at",
        ],
        values,
        "id",
        "id",
    )
    .await?;
    Ok(())
}

fn day_for_timestamp(timestamp: i64) -> String {
    chrono::DateTime::from_timestamp_millis(timestamp)
        .map(|value| value.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "1970-01-01".into())
}

fn activity_id(application_id: &str, environment_id: &str, device_hash: &str, day: &str) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(b"sonde:device-activity-day\0");
    for value in [application_id, environment_id, device_hash, day] {
        hasher.update(value.as_bytes());
        hasher.update(b"\0");
    }
    hex::encode(hasher.finalize())
}

fn bounded_string(name: &str, length: u32) -> ColumnDef {
    ColumnDef::new(Alias::new(name))
        .string_len(length)
        .not_null()
        .to_owned()
}
