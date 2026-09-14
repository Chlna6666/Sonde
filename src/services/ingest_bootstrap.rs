use sea_orm::{DatabaseConnection, DbErr};

use crate::{database::ingest_bootstrap, error::AppError, security::hmac_sha256};

const MINUTE_MILLIS: i64 = 60_000;
const HOUR_MILLIS: i64 = 60 * MINUTE_MILLIS;
const PERSISTENCE_GRACE_MILLIS: i64 = 5 * MINUTE_MILLIS;
const IP_TOKEN_REQUESTS_PER_MINUTE: i64 = 30;
const DEVICE_TOKEN_REQUESTS_PER_MINUTE: i64 = 8;
const NEW_DEVICES_PER_IP_PER_HOUR: i64 = 128;
const OPAQUE_KEY_CONTEXT: &[u8] = b"sonde-ingest-bootstrap-key\0";

pub(crate) async fn charge_ip_token(
    database: &DatabaseConnection,
    pepper: &[u8],
    client_ip: &str,
) -> Result<bool, DbErr> {
    charge_ip_token_at(
        database,
        pepper,
        client_ip,
        chrono::Utc::now().timestamp_millis(),
    )
    .await
}

pub(crate) async fn check_device_enrollment(
    database: &DatabaseConnection,
    pepper: &[u8],
    client_ip: &str,
    application_id: &str,
    device_id: &str,
) -> Result<bool, DbErr> {
    check_device_enrollment_at(
        database,
        pepper,
        client_ip,
        application_id,
        device_id,
        chrono::Utc::now().timestamp_millis(),
    )
    .await
}

pub(crate) async fn charge_device_token(
    database: &DatabaseConnection,
    pepper: &[u8],
    application_id: &str,
    device_id: &str,
    cost: u64,
) -> Result<bool, DbErr> {
    charge_device_token_at(
        database,
        pepper,
        application_id,
        device_id,
        cost,
        chrono::Utc::now().timestamp_millis(),
    )
    .await
}

async fn charge_ip_token_at(
    database: &DatabaseConnection,
    pepper: &[u8],
    client_ip: &str,
    now: i64,
) -> Result<bool, DbErr> {
    let (window_id, expires_at) = fixed_window(now, MINUTE_MILLIS);
    let bucket_key = opaque_key(pepper, b"ip-token", &[client_ip], window_id)
        .map_err(|error| DbErr::Custom(error.to_string()))?;
    ingest_bootstrap::charge_window(
        database,
        &bucket_key,
        1,
        IP_TOKEN_REQUESTS_PER_MINUTE,
        expires_at,
    )
    .await
}

async fn check_device_enrollment_at(
    database: &DatabaseConnection,
    pepper: &[u8],
    client_ip: &str,
    application_id: &str,
    device_id: &str,
    now: i64,
) -> Result<bool, DbErr> {
    let (window_id, expires_at) = fixed_window(now, HOUR_MILLIS);
    let budget_key = opaque_key(
        pepper,
        b"device-enrollment-budget",
        &[client_ip, application_id],
        window_id,
    )
    .map_err(|error| DbErr::Custom(error.to_string()))?;
    let enrollment_key = opaque_key(
        pepper,
        b"device-enrollment",
        &[client_ip, application_id, device_id],
        window_id,
    )
    .map_err(|error| DbErr::Custom(error.to_string()))?;
    ingest_bootstrap::record_enrollment_with_budget(
        database,
        &enrollment_key,
        &budget_key,
        NEW_DEVICES_PER_IP_PER_HOUR,
        expires_at,
    )
    .await
}

async fn charge_device_token_at(
    database: &DatabaseConnection,
    pepper: &[u8],
    application_id: &str,
    device_id: &str,
    cost: u64,
    now: i64,
) -> Result<bool, DbErr> {
    let cost = i64::try_from(std::cmp::max(cost, 1)).unwrap_or(i64::MAX);
    let (window_id, expires_at) = fixed_window(now, MINUTE_MILLIS);
    let bucket_key = opaque_key(
        pepper,
        b"device-token",
        &[application_id, device_id],
        window_id,
    )
    .map_err(|error| DbErr::Custom(error.to_string()))?;
    ingest_bootstrap::charge_window(
        database,
        &bucket_key,
        cost,
        DEVICE_TOKEN_REQUESTS_PER_MINUTE,
        expires_at,
    )
    .await
}

fn fixed_window(now: i64, window_millis: i64) -> (i64, i64) {
    let window_id = now.div_euclid(window_millis);
    let start = window_id.saturating_mul(window_millis);
    let window_end = start.saturating_add(window_millis);
    (
        window_id,
        window_end.saturating_add(PERSISTENCE_GRACE_MILLIS),
    )
}

fn opaque_key(
    pepper: &[u8],
    purpose: &[u8],
    parts: &[&str],
    window_id: i64,
) -> Result<String, AppError> {
    let payload_capacity = OPAQUE_KEY_CONTEXT.len()
        + purpose.len()
        + 1
        + std::mem::size_of::<i64>()
        + parts.iter().map(|part| 8 + part.len()).sum::<usize>();
    let mut payload = Vec::with_capacity(payload_capacity);
    payload.extend_from_slice(OPAQUE_KEY_CONTEXT);
    payload.extend_from_slice(purpose);
    payload.push(0);
    payload.extend_from_slice(&window_id.to_be_bytes());
    for part in parts {
        payload.extend_from_slice(&(part.len() as u64).to_be_bytes());
        payload.extend_from_slice(part.as_bytes());
    }
    Ok(hex::encode(hmac_sha256(pepper, &payload)?))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::database;

    #[test]
    fn opaque_keys_hide_raw_bootstrap_identifiers() {
        let key = opaque_key(
            b"test-secret-pepper-32-bytes-long!",
            b"device-token",
            &["203.0.113.44", "app-1", "device-visible-value"],
            1234,
        )
        .unwrap();
        assert_eq!(key.len(), 64);
        assert!(!key.contains("203.0.113.44"));
        assert!(!key.contains("device-visible-value"));
    }

    #[tokio::test]
    async fn ip_token_budget_is_shared_across_database_connections() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("bootstrap-budget.sqlite");
        let url = format!(
            "sqlite://{}?mode=rwc",
            path.to_string_lossy().replace('\\', "/")
        );
        let replica_a = database::connect(&url).await.unwrap();
        database::migrate(&replica_a).await.unwrap();
        let replica_b = database::connect(&url).await.unwrap();
        let pepper = b"test-secret-pepper-32-bytes-long!";
        let now = 1_800_000_i64;

        for index in 0..IP_TOKEN_REQUESTS_PER_MINUTE {
            let database = if index % 2 == 0 {
                &replica_a
            } else {
                &replica_b
            };
            assert!(
                charge_ip_token_at(database, pepper, "203.0.113.10", now)
                    .await
                    .unwrap()
            );
        }
        assert!(
            !charge_ip_token_at(&replica_b, pepper, "203.0.113.10", now)
                .await
                .unwrap()
        );
        assert!(
            charge_ip_token_at(&replica_b, pepper, "203.0.113.10", now + MINUTE_MILLIS,)
                .await
                .unwrap()
        );
    }
}
