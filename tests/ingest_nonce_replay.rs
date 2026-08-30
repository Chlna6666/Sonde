use sonde::database::{self, ingest_nonce_repo};

#[tokio::test]
async fn nonce_replay_is_rejected_across_independent_database_connections(
) -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("nonce-replay.sqlite");
    let database_url = format!("sqlite://{}?mode=rwc", path.to_string_lossy().replace('\\', "/"));

    let replica_a = database::connect(&database_url).await?;
    database::migrate(&replica_a).await?;
    let replica_b = database::connect(&database_url).await?;

    let now = chrono::Utc::now().timestamp_millis();
    let expires_at = now + 120_000;
    assert!(
        ingest_nonce_repo::record_once(&replica_a, "token-a", "nonce-a", expires_at).await?
    );
    assert!(
        !ingest_nonce_repo::record_once(&replica_b, "token-a", "nonce-a", expires_at).await?
    );
    assert!(
        ingest_nonce_repo::record_once(&replica_b, "token-a", "nonce-b", expires_at).await?
    );

    assert_eq!(ingest_nonce_repo::cleanup_expired(&replica_a, now).await?, 0);
    assert_eq!(
        ingest_nonce_repo::cleanup_expired(&replica_a, expires_at).await?,
        2
    );
    assert!(
        ingest_nonce_repo::record_once(
            &replica_b,
            "token-a",
            "nonce-a",
            expires_at + 120_000,
        )
        .await?
    );

    Ok(())
}
