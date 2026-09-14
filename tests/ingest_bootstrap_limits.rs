use sonde::database::{self, ingest_bootstrap};

#[tokio::test]
async fn fixed_window_budget_is_atomic_across_connections() -> Result<(), Box<dyn std::error::Error>>
{
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("bootstrap-rate.sqlite");
    let database_url = format!(
        "sqlite://{}?mode=rwc",
        path.to_string_lossy().replace('\\', "/")
    );
    let replica_a = database::connect(&database_url).await?;
    database::migrate(&replica_a).await?;
    let replica_b = database::connect(&database_url).await?;

    let key = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let expires_at = chrono::Utc::now().timestamp_millis() + 60_000;
    assert!(ingest_bootstrap::charge_window(&replica_a, key, 1, 2, expires_at).await?);
    assert!(ingest_bootstrap::charge_window(&replica_b, key, 1, 2, expires_at).await?);
    assert!(!ingest_bootstrap::charge_window(&replica_a, key, 1, 2, expires_at).await?);

    Ok(())
}

#[tokio::test]
async fn repeated_device_enrollment_does_not_consume_distinct_device_budget()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("bootstrap-enrollment.sqlite");
    let database_url = format!(
        "sqlite://{}?mode=rwc",
        path.to_string_lossy().replace('\\', "/")
    );
    let replica_a = database::connect(&database_url).await?;
    database::migrate(&replica_a).await?;
    let replica_b = database::connect(&database_url).await?;

    let budget_key = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let enrollment_a = "1111111111111111111111111111111111111111111111111111111111111111";
    let enrollment_b = "2222222222222222222222222222222222222222222222222222222222222222";
    let enrollment_c = "3333333333333333333333333333333333333333333333333333333333333333";
    let expires_at = chrono::Utc::now().timestamp_millis() + 3_600_000;

    assert!(
        ingest_bootstrap::record_enrollment_with_budget(
            &replica_a,
            enrollment_a,
            budget_key,
            2,
            expires_at,
        )
        .await?
    );
    assert!(
        ingest_bootstrap::record_enrollment_with_budget(
            &replica_b,
            enrollment_a,
            budget_key,
            2,
            expires_at,
        )
        .await?
    );
    assert!(
        ingest_bootstrap::record_enrollment_with_budget(
            &replica_b,
            enrollment_b,
            budget_key,
            2,
            expires_at,
        )
        .await?
    );
    assert!(
        !ingest_bootstrap::record_enrollment_with_budget(
            &replica_a,
            enrollment_c,
            budget_key,
            2,
            expires_at,
        )
        .await?
    );
    assert!(
        !ingest_bootstrap::record_enrollment_with_budget(
            &replica_b,
            enrollment_c,
            budget_key,
            2,
            expires_at,
        )
        .await?
    );

    Ok(())
}
