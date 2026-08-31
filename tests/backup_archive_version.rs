use std::error::Error;

use sha2::{Digest, Sha256};
use sonde::database::backup_archive::{self, BackupEnd, BackupManifest, BackupRecord};

#[tokio::test]
async fn current_validator_accepts_20_and_21_but_rejects_future_versions(
) -> Result<(), Box<dyn Error>> {
    assert_eq!(backup_archive::FORMAT_VERSION, "2.1");

    for version in ["2.0", "2.1"] {
        let archive = tempfile::NamedTempFile::new()?;
        write_manifest_only_archive(archive.path(), version).await?;
        let manifest = backup_archive::validate_backup_file(archive.path()).await?;
        assert_eq!(manifest.format_version, version);
    }

    let future = tempfile::NamedTempFile::new()?;
    write_manifest_only_archive(future.path(), "2.2").await?;
    assert!(backup_archive::validate_backup_file(future.path()).await.is_err());
    Ok(())
}

async fn write_manifest_only_archive(
    path: &std::path::Path,
    version: &str,
) -> Result<(), Box<dyn Error>> {
    let manifest = BackupRecord::Manifest(BackupManifest {
        format_version: version.into(),
        backup_type: backup_archive::BACKUP_TYPE.into(),
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
    let end = BackupRecord::End(BackupEnd {
        records: 0,
        sha256: hex::encode(digest.finalize()),
    });
    let end_line = line(&end)?;

    let mut bytes = Vec::with_capacity(manifest_line.len() + end_line.len());
    bytes.extend_from_slice(&manifest_line);
    bytes.extend_from_slice(&end_line);
    tokio::fs::write(path, bytes).await?;
    Ok(())
}

fn line(record: &BackupRecord) -> Result<Vec<u8>, serde_json::Error> {
    let mut bytes = serde_json::to_vec(record)?;
    bytes.push(b'\n');
    Ok(bytes)
}
