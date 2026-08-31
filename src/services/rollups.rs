use std::time::Duration;

use sea_orm::{DatabaseConnection, DbErr};
use tracing::{info, warn};

use crate::database::{dimension_rollup, first_seen, log_error_rollup, rollups, user_rollup};

const ROLLUP_INTERVAL: Duration = Duration::from_secs(2);
const ROLLUP_SETTLE_MILLIS: i64 = 2_000;
const ROLLUP_BATCH_SIZE: u64 = 32;

pub fn spawn_rollup_worker(database: DatabaseConnection) {
    tokio::spawn(async move {
        match rollups::seed_historical_dirty_days_once(&database).await {
            Ok(seed_count) if seed_count > 0 => {
                info!(scope_days = seed_count, "seeded historical telemetry rollup work");
            }
            Ok(_) => {}
            Err(error) => {
                warn!(error = %error, "failed to seed historical telemetry rollups");
            }
        }

        match dimension_rollup::seed_historical_dimension_dirty_days_once(&database).await {
            Ok(seed_count) if seed_count > 0 => {
                info!(scope_days = seed_count, "seeded historical telemetry dimension rollup work");
            }
            Ok(_) => {}
            Err(error) => {
                warn!(error = %error, "failed to seed historical telemetry dimension rollups");
            }
        }
        match user_rollup::seed_historical_user_dirty_days_once(&database).await {
            Ok(seed_count) if seed_count > 0 => {
                info!(scope_days = seed_count, "seeded historical telemetry user-set rollup work");
            }
            Ok(_) => {}
            Err(error) => {
                warn!(error = %error, "failed to seed historical telemetry user-set rollups");
            }
        }
        match log_error_rollup::seed_historical_dirty_days_once(&database).await {
            Ok(seed_count) if seed_count > 0 => {
                info!(scope_days = seed_count, "seeded historical log error rollup work");
            }
            Ok(_) => {}
            Err(error) => {
                warn!(error = %error, "failed to seed historical log error rollups");
            }
        }

        let mut interval = tokio::time::interval(ROLLUP_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            match process_ready_rollups(&database).await {
                Ok(processed) if processed > 0 => {
                    info!(processed, "telemetry daily rollups refreshed");
                }
                Ok(_) => {}
                Err(error) => {
                    warn!(error = %error, "telemetry rollup worker encountered an error");
                }
            }
        }
    });
}

async fn process_ready_rollups(database: &DatabaseConnection) -> Result<usize, DbErr> {
    let marked_before = chrono::Utc::now()
        .timestamp_millis()
        .saturating_sub(ROLLUP_SETTLE_MILLIS);
    let dirty_days = rollups::list_dirty_days(database, ROLLUP_BATCH_SIZE, marked_before).await?;
    let mut processed = 0_usize;
    for dirty in dirty_days {
        // Dimensions, first-seen and exact daily user sets are all event-derived. Metric/log/error
        // ingestion must not force those comparatively expensive scans or make their query paths
        // fall back to raw events.
        if dirty.has_source(rollups::DIRTY_SOURCE_EVENT) {
            if !dimension_rollup::recompute_claimed_day_dimensions(database, &dirty).await? {
                tokio::task::yield_now().await;
                continue;
            }
            if !first_seen::refresh_dirty_day(database, &dirty).await? {
                tokio::task::yield_now().await;
                continue;
            }
            if !user_rollup::recompute_claimed_day_user_set(database, &dirty).await? {
                tokio::task::yield_now().await;
                continue;
            }
        }
        if dirty.has_source(log_error_rollup::DIRTY_SOURCE_LOG_ERROR)
            && !log_error_rollup::recompute_claimed_day(database, &dirty).await?
        {
            tokio::task::yield_now().await;
            continue;
        }
        if rollups::recompute_claimed_day(database, dirty).await? {
            processed = processed.saturating_add(1);
        }
        tokio::task::yield_now().await;
    }
    Ok(processed)
}
