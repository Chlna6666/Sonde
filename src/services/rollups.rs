use std::time::Duration;

use sea_orm::{DatabaseConnection, DbErr};
use tracing::{info, warn};

use crate::database::rollup_repo;

const ROLLUP_INTERVAL: Duration = Duration::from_secs(2);
const ROLLUP_SETTLE_MILLIS: i64 = 2_000;
const ROLLUP_BATCH_SIZE: u64 = 32;

pub fn spawn_rollup_worker(database: DatabaseConnection) {
    tokio::spawn(async move {
        match rollup_repo::seed_historical_dirty_days_once(&database).await {
            Ok(seed_count) if seed_count > 0 => {
                info!(scope_days = seed_count, "seeded historical telemetry rollup work");
            }
            Ok(_) => {}
            Err(error) => {
                warn!(error = %error, "failed to seed historical telemetry rollups");
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
    let dirty_days =
        rollup_repo::list_dirty_days(database, ROLLUP_BATCH_SIZE, marked_before).await?;
    let mut processed = 0_usize;
    for dirty in dirty_days {
        if rollup_repo::recompute_claimed_day(database, dirty).await? {
            processed = processed.saturating_add(1);
        }
        tokio::task::yield_now().await;
    }
    Ok(processed)
}
