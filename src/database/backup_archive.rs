use super::backup_models as backup_repo;

#[path = "backup_archive_format.rs"]
mod format;

pub use format::{
    BACKUP_TYPE, CONTENT_TYPE, FILE_EXTENSION, FORMAT_VERSION, MAX_RECORD_BYTES,
    BackupAlertDelivery, BackupDailyRollup, BackupErrorGroup, BackupErrorOccurrence, BackupImportRun,
    BackupV2End as BackupEnd, BackupV2Error as BackupError, BackupV2Manifest as BackupManifest,
    BackupMetricPointV2 as BackupMetricPoint, BackupV2Record as BackupRecord,
    export_full_system_stream, manifest_summary, validate_backup_file,
};

// Keep the implementation's historical identifiers private to the database layer while the public
// archive API uses version-neutral names. Format versions remain data compatibility markers only.
pub(crate) use format::{BackupMetricPointV2, BackupV2Error, BackupV2Record};
