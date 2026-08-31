#![allow(clippy::unwrap_used)]

use std::error::Error;

use sha2::{Digest, Sha256};
use sonde::database::{
    self, app_repo,
    backup_archive_repo::{
        self, BackupEnd, BackupManifest, BackupMetricPoint, BackupRecord,
    },
    backup_archive_validation_repo,
};

#[tokio::test]
async fn semantic_histogram_failure_does_not_modify_restore_target() -> Result<(), Box<dyn Error>> {
    let archive = tempfile::NamedTempFile::new()?;
    write_archive(archive.path(), vec![invalid_histogram_record()]).await?;

    // Envelope validation only verifies framing/manifest/count/digest. This proves the file reaches
    // the semantic layer rather than failing because the test archive itself is structurally corrupt.
    let manifest = backup_archive_repo::validate_backup_file(archive.path()).await?;
    assert_eq!(manifest.format_version, backup_archive_repo::FORMAT_VERSION);
    assert!(
        backup_archive_validation_repo::validate_backup_file_semantics(archive.path())
            .await
            .is_err()
    );

    let target = database::connect("sqlite::memory:").await?;
    database::migrate(&target).await?;
    let (application_id, _) =
        app_repo::create_application(&target, "Keep Me", "keep-me", None).await?;

    let result = backup_archive_validation_repo::restore_full_system_exact_validated(
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

#[tokio::test]
async fn duplicate_record_ids_are_rejected_even_with_valid_digest() -> Result<(), Box<dyn Error>> {
    let archive = tempfile::NamedTempFile::new()?;
    write_archive(
        archive.path(),
        vec![scalar_metric_record("metric-1"), scalar_metric_record("metric-1")],
    )
    .await?;

    assert!(backup_archive_repo::validate_backup_file(archive.path()).await.is_ok());
    assert!(
        backup_archive_validation_repo::validate_backup_file_semantics(archive.path())
            .await
            .is_err()
    );
    Ok(())
}

fn invalid_histogram_record() -> BackupRecord {
    BackupRecord::MetricPoint(BackupMetricPoint {
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
    })
}

fn scalar_metric_record(id: &str) -> BackupRecord {
    BackupRecord::MetricPoint(BackupMetricPoint {
        id: id.into(),
        application_id: "app-1".into(),
        environment_id: "prod".into(),
        name: "cpu.usage".into(),
        metric_type: "gauge".into(),
        value: 0.5,
        unit: Some("ratio".into()),
        timestamp: 1_777_680_000_000,
        attributes: "{}".into(),
        received_at: 1_777_680_000_000,
        histogram_count: None,
        histogram_sum: None,
        histogram_min: None,
        histogram_max: None,
        histogram_bounds: None,
        histogram_bucket_counts: None,
    })
}

async fn write_archive(
    path: &std::path::Path,
    records: Vec<BackupRecord>,
) -> Result<(), Box<dyn Error>> {
    let manifest = BackupRecord::Manifest(BackupManifest {
        format_version: backup_archive_repo::FORMAT_VERSION.into(),
        backup_type: backup_archive_repo::BACKUP_TYPE.into(),
        exported_at: 1_777_680_000_000,
        server_version: "test".into(),
        contains_secrets: false,
        totp_secrets_included: false,
        ephemeral_auth_state_included: false,
        generated_rollups_included: false,
    });
    let manifest_line = line(&manifest)?;
    let mut digest = Sha256::new();
    digest.update(&manifest_line);
    let mut bytes = manifest_line;

    let record_count = records.len() as u64;
    for record in records {
        let record_line = line(&record)?;
        digest.update(&record_line);
        bytes.extend_from_slice(&record_line);
    }

    let end = BackupRecord::End(BackupEnd {
        records: record_count,
        sha256: hex::encode(digest.finalize()),
    });
    bytes.extend_from_slice(&line(&end)?);
    tokio::fs::write(path, bytes).await?;
    Ok(())
}

fn line(record: &BackupRecord) -> Result<Vec<u8>, serde_json::Error> {
    let mut bytes = serde_json::to_vec(record)?;
    bytes.push(b'\n');
    Ok(bytes)
}
