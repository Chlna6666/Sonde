#[path = "backup_archive_validation_impl.rs"]
mod implementation;

pub use implementation::{restore_full_system_exact_validated, validate_backup_file_semantics};
