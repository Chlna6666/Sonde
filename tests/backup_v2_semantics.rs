#![allow(clippy::unwrap_used)]

use std::error::Error;

use sea_orm::ConnectionTrait;
use sha2::{Digest, Sha256};
use sonde::database::{
    self, app_repo,
    backup_v2_repo::{
        self, BackupMetricPointV2, BackupV2End, BackupV2Manifest, BackupV2Record,
    },
    backup_v2_validation_repo,
};

#[tokio::test]
async fn semantic_histogram_failure_does_not_modify_restore_target() -> Result<(), Box<dyn Error>> {
    let archive = tempfile::NamedTempFile::new()?;
    write_invalid_histogram_archive(archive.path()).await?;

    // The regular v2 validator only verifies framing/manifest/count/digest, so this proves the file
    // reaches the new semantic layer rather than failing because the test archive itself is corrupt.
    let manifest = backup_v2_repo::validate_backup_file(archive.path()).await?;
    assert_eq!(manifest.format_version, backup_v2_repo::FORMAT_VERSION);
    assert!(
        backup_v2_validation_repo::validate_backup_file_semantics(archive.path())
            .await
            .is_err()
    );

    let target = database::connect("sqlite::memory:").await?;
    database::migrate(&target).await?;
    let (application_id, _) =
        app_repo::create_application(&target, "Keep Me", "keep-me", None).await?;

    let result = backup_v2_validation_repo::restore_full_system_exact_validated(
        &target,
        archive.path(),
    )
    .await;
    assert!(result.is_err());

    let applications = app_repo::list_applications(&target, None, true).await?;
    assert_eq!(applications.len(), 1);
    assert_eq!(applications[0].id, application_id);
    assert_eq!(applications[0].slug, "keep-me");
    Ok(())
}

async fn write_invalid_histogram_archive(path: &std::path::Path) -> Result<(), Box<dyn Error>> {
    let manifest = BackupV2Record::Manifest(BackupV2Manifest {
        format_version: backup_v2_repo::FORMAT_VERSION.into(),
        backup_type: backup_v2_repo::BACKUP_TYPE.into(),
        exported_at: 1_777_680_000_000,
        server_version: "test".into(),
        contains_secrets: false,
        totp_secrets_included: false,
        ephemeral_auth_state_included: false,
        generated_rollups_included: false,
    });
    let metric = BackupV2Record::MetricPoint(BackupMetricPointV2 {
        id: "metric-1".into(),
        application_id: "app-1".into(),
        environment_id: "prod".into(),
        name: "http.duration".into(),
        metric_type: "histogram".into(),
        value: 10.0,
        unit: Some("ms".into()),
        timestamp: 1_777_680_000_000,
        attributes: "{}".into(),
        received_at: 1_777_680_000_000,
        histogram_count: Some(4),
        histogram_sum: Some(40.0),
        histogram_min: Some(1.0),
        histogram_max: Some(20.0),
        histogram_bounds: Some("[5.0,10.0]".into()),
        // Three buckets are structurally correct for two bounds, but their population sums to 3,
        // not histogram_count=4. SHA integrity therefore cannot catch this semantic corruption.
        histogram_bucket_counts: Some("[1,1,1]".into()),
    });

    let manifest_line = line(&manifest)?;
    let metric_line = line(&metric)?;
    let mut digest = Sha256::new();
    digest.update(&manifest_line);
    digest.update(&metric_line);
    let end = BackupV2Record::End(BackupV2End {
        records: 1,
        sha256: hex::encode(digest.finalize()),
    });
    let end_line = line(&end)?;

    let mut bytes = Vec::with_capacity(manifest_line.len() + metric_line.len() + end_line.len());
    bytes.extend_from_slice(&manifest_line);
    bytes.extend_from_slice(&metric_line);
    bytes.extend_from_slice(&end_line);
    tokio::fs::write(path, bytes).await?;
    Ok(())
}

fn line(record: &BackupV2Record) -> Result<Vec<u8>, serde_json::Error> {
    let mut bytes = serde_json::to_vec(record)?;
    bytes.push(b'\n');
    Ok(bytes)
}

#[allow(dead_code)]
async fn _assert_database_connection_trait(database: &sea_orm::DatabaseConnection) {
    let _ = database.get_database_backend();
}
