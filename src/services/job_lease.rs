use std::{future::Future, time::Duration};

use sea_orm::{DatabaseConnection, DbErr};

use crate::database::job_lease;

pub async fn run_with_lease<T, F, Fut>(
    database: &DatabaseConnection,
    lease_name: &str,
    holder_id: &str,
    ttl: Duration,
    operation: F,
) -> Result<Option<T>, DbErr>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T, DbErr>>,
{
    let ttl_millis = i64::try_from(ttl.as_millis()).unwrap_or(i64::MAX);
    if !job_lease::try_acquire_or_renew(database, lease_name, holder_id, ttl_millis).await? {
        return Ok(None);
    }

    let heartbeat_millis = (ttl.as_millis() / 3).clamp(1_000, u128::from(u64::MAX));
    let heartbeat_period = Duration::from_millis(heartbeat_millis as u64);
    let start = tokio::time::Instant::now() + heartbeat_period;
    let mut heartbeat = tokio::time::interval_at(start, heartbeat_period);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let operation = operation();
    tokio::pin!(operation);

    loop {
        tokio::select! {
            result = &mut operation => return result.map(Some),
            _ = heartbeat.tick() => {
                if !job_lease::try_acquire_or_renew(
                    database,
                    lease_name,
                    holder_id,
                    ttl_millis,
                )
                .await?
                {
                    // Dropping the pinned future cancels the local task if another instance owns
                    // the lease. Database statements already committed before this point remain
                    // valid, while the rest of the iteration is prevented from overlapping.
                    return Ok(None);
                }
            }
        }
    }
}
