use std::time::Duration;

use sea_orm::DatabaseConnection;
use tracing::{info, warn};
use uuid::Uuid;

use crate::services::{alerts, job_lease, retention};

const ALERT_INTERVAL: Duration = Duration::from_secs(30);
const ALERT_LEASE_TTL: Duration = Duration::from_secs(90);
const RETENTION_INTERVAL: Duration = Duration::from_secs(4 * 60 * 60);
const RETENTION_LEASE_TTL: Duration = Duration::from_secs(4 * 60 * 60 + 5 * 60);

pub fn spawn_leased_workers(database: DatabaseConnection) {
    spawn_alert_worker(database.clone());
    spawn_retention_worker(database);
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
                || retention::run_retention_sweep(&database),
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
