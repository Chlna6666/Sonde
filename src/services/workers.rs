use std::time::Duration;

use sea_orm::{DatabaseConnection, DbErr};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    database::{alert_delivery_repo, device_risk_repo, first_seen_repo},
    services::{alerts, job_lease, retention},
};

const ALERT_INTERVAL: Duration = Duration::from_secs(30);
const ALERT_LEASE_TTL: Duration = Duration::from_secs(90);
const ALERT_DELIVERY_INTERVAL: Duration = Duration::from_secs(1);
const ALERT_DELIVERY_LEASE_TTL: Duration = Duration::from_secs(90);
const ALERT_DELIVERY_BATCH: u64 = 4;
const ALERT_DELIVERY_HISTORY_RETENTION_MILLIS: i64 = 90 * 86_400_000;
const ALERT_DELIVERY_HISTORY_PRUNE_MAX: u64 = 5_000;
const RETENTION_INTERVAL: Duration = Duration::from_secs(4 * 60 * 60);
const RETENTION_LEASE_TTL: Duration = Duration::from_secs(4 * 60 * 60 + 5 * 60);
const DEVICE_RISK_DECAY_QUIET_MILLIS: i64 = 4 * 60 * 60 * 1_000;
const FIRST_SEEN_BACKFILL_INTERVAL: Duration = Duration::from_secs(10);
const FIRST_SEEN_BACKFILL_LEASE_TTL: Duration = Duration::from_secs(60);
const FIRST_SEEN_BACKFILL_BATCH: u64 = 32;

pub fn spawn_leased_workers(database: DatabaseConnection) {
    spawn_alert_worker(database.clone());
    spawn_alert_delivery_worker(database.clone());
    spawn_retention_worker(database.clone());
    spawn_first_seen_backfill_worker(database);
}

fn spawn_alert_worker(database: DatabaseConnection) {
    let holder_id = format!("alert:{}", Uuid::now_v7());
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(ALERT_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            match job_lease::run_with_lease(
                &database,
                "alert-evaluator-v1",
                &holder_id,
                ALERT_LEASE_TTL,
                || alerts::evaluate_all_rules(&database),
            )
            .await
            {
                Ok(Some(evaluated)) if evaluated > 0 => {
                    info!(evaluated, "leased alert evaluation completed");
                }
                Ok(_) => {}
                Err(error) => {
                    warn!(error = %error, "leased alert evaluator encountered an error");
                }
            }
        }
    });
}

fn spawn_alert_delivery_worker(database: DatabaseConnection) {
    let holder_id = format!("alert-delivery:{}", Uuid::now_v7());
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(ALERT_DELIVERY_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            match job_lease::run_with_lease(
                &database,
                "alert-delivery-v1",
                &holder_id,
                ALERT_DELIVERY_LEASE_TTL,
                || alerts::process_due_deliveries(&database, ALERT_DELIVERY_BATCH),
            )
            .await
            {
                Ok(Some(processed)) if processed > 0 => {
                    info!(processed, "leased alert delivery batch completed");
                }
                Ok(_) => {}
                Err(error) => {
                    warn!(error = %error, "leased alert delivery worker encountered an error");
                }
            }
        }
    });
}

fn spawn_retention_worker(database: DatabaseConnection) {
    let holder_id = format!("retention:{}", Uuid::now_v7());
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(RETENTION_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            match job_lease::run_with_lease(
                &database,
                "retention-sweep-v1",
                &holder_id,
                RETENTION_LEASE_TTL,
                || run_retention_cycle(&database),
            )
            .await
            {
                Ok(Some(report)) => {
                    info!(apps = report.apps_processed, "leased retention sweep completed");
                }
                Ok(None) => {}
                Err(error) => {
                    warn!(error = %error, "leased retention worker encountered an error");
                }
            }
        }
    });
}

async fn run_retention_cycle(
    database: &DatabaseConnection,
) -> Result<retention::RetentionReport, DbErr> {
    let report = retention::run_retention_sweep(database).await?;
    let now = chrono::Utc::now().timestamp_millis();
    let cutoff = now.saturating_sub(ALERT_DELIVERY_HISTORY_RETENTION_MILLIS);
    let pruned = alert_delivery_repo::prune_terminal_before(
        database,
        cutoff,
        ALERT_DELIVERY_HISTORY_PRUNE_MAX,
    )
    .await?;
    if pruned > 0 {
        info!(pruned, "pruned terminal alert delivery history");
    }

    let risk_cutoff = now.saturating_sub(DEVICE_RISK_DECAY_QUIET_MILLIS);
    let decayed = device_risk_repo::decay_scores(database, risk_cutoff).await?;
    if decayed > 0 {
        info!(devices = decayed, "decayed device abuse risk after quiet period");
    }
    Ok(report)
}

fn spawn_first_seen_backfill_worker(database: DatabaseConnection) {
    let holder_id = format!("first-seen:{}", Uuid::now_v7());
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(FIRST_SEEN_BACKFILL_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            match job_lease::run_with_lease(
                &database,
                "first-seen-backfill-v1",
                &holder_id,
                FIRST_SEEN_BACKFILL_LEASE_TTL,
                || first_seen_repo::run_backfill_batch(&database, FIRST_SEEN_BACKFILL_BATCH),
            )
            .await
            {
                Ok(Some(processed)) if processed > 0 => {
                    info!(processed, "leased first-seen backfill batch completed");
                }
                Ok(_) => {}
                Err(error) => {
                    warn!(error = %error, "leased first-seen backfill worker encountered an error");
                }
            }
        }
    });
}
