use sea_orm::{ConnectionTrait, QueryResult};
use sea_orm_migration::prelude::*;

use crate::database::query::insert_batch_ignore_conflicts;

use super::columns::{bigint, create_index, create_table};

pub(super) struct DeviceActivityHours;

impl MigrationName for DeviceActivityHours {
    fn name(&self) -> &str {
        "m20260901_000027_device_activity_hours"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for DeviceActivityHours {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            manager,
            "telemetry_device_activity_hours",
            vec![
                bounded_string("id", 64).primary_key().to_owned(),
                bounded_string("application_id", 64),
                bounded_string("environment_id", 64),
                bounded_string("device_hash", 64),
                bounded_string("hour", 16),
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
            "idx_device_activity_hour_scope",
            "telemetry_device_activity_hours",
            &["application_id", "environment_id", "hour"],
            false,
        )
        .await?;
        create_index(
            manager,
            "idx_device_activity_hour_device",
            "telemetry_device_activity_hours",
            &["application_id", "environment_id", "device_hash", "hour"],
            true,
        )
        .await?;
        backfill_existing_profiles(manager).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(Alias::new("telemetry_device_activity_hours"))
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
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
        let hour = hour_for_timestamp(last_seen_at);
        values.push(vec![
            activity_id(&application_id, &environment_id, &device_hash, &hour).into(),
            application_id.into(),
            environment_id.into(),
            device_hash.into(),
            hour.into(),
            last_seen_at.into(),
            last_seen_at.into(),
            0_i64.into(),
            1_i64.into(),
            last_seen_at.into(),
        ]);
    }
    insert_batch_ignore_conflicts(
        database,
        "telemetry_device_activity_hours",
        &[
            "id",
            "application_id",
            "environment_id",
            "device_hash",
            "hour",
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

fn hour_for_timestamp(timestamp: i64) -> String {
    chrono::DateTime::from_timestamp_millis(timestamp)
        .map(|value| value.format("%Y-%m-%d %H:00").to_string())
        .unwrap_or_else(|| "1970-01-01 00:00".into())
}

fn activity_id(
    application_id: &str,
    environment_id: &str,
    device_hash: &str,
    hour: &str,
) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(b"sonde:device-activity-hour\0");
    for value in [application_id, environment_id, device_hash, hour] {
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
