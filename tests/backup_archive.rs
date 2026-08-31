mod sonde {
    pub use ::sonde::domain;

    pub mod database {
        pub use ::sonde::database::*;
        pub use ::sonde::database::applications as app_repo;
        pub use ::sonde::database::auth as auth_repo;
        pub use ::sonde::database::backup_archive as backup_archive_repo;
        pub use ::sonde::database::backup_restore as backup_archive_restore_repo;
        pub use ::sonde::database::dimension_restore as dimension_restore_repo;
        pub use ::sonde::database::telemetry as telemetry_repo;
        pub use ::sonde::database::telemetry_count as telemetry_count_repo;
    }
}

#[path = "backup_archive/round_trip.rs"]
mod round_trip;
