use crate::{
    database::stats_repo::{self, AppTelemetryStats, Overview},
    error::AppError,
    services::authentication::AuthenticatedUser,
    state::InstalledState,
};

pub async fn overview(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    days: Option<u32>,
) -> Result<Overview, AppError> {
    user.require("telemetry.read", None)?;
    Ok(stats_repo::overview(&installed.database, days).await?)
}

pub async fn application_stats(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    application_id: &str,
    environment_id: Option<&str>,
    days: Option<u32>,
) -> Result<AppTelemetryStats, AppError> {
    user.require("telemetry.read", Some(application_id))?;
    Ok(
        stats_repo::application_stats(&installed.database, application_id, environment_id, days)
            .await?,
    )
}
