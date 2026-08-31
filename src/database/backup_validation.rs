use super::{
    backup_archive_repo as backup_v2_repo,
    backup_archive_restore_repo as backup_v2_restore_repo,
};

#[path = "backup_archive_validation_impl.rs"]
mod implementation;

pub use implementation::{restore_full_system_exact_validated, validate_backup_file_semantics};
