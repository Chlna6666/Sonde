// Integration-test crate roots cannot use standard module discovery for files kept in a
// same-named subdirectory without making those files separate Cargo integration tests.
#[path = "backup_archive/round_trip.rs"]
mod round_trip;
