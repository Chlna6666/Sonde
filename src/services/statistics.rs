use crate::{
    database::stats_repo::{self, AppTelemetryStats, Overview},
    error::AppError,
    services::authentication::AuthenticatedUser,
    state::InstalledState,
};

const MAX_STATISTICS_DAYS: u32 = 730;

pub fn validate_days(days: Option<u32>) -> Result<Option<u32>, AppError> {
    match days {
        Some(0) => Err(AppError::Validation(
            "statistics days must be greater than zero".into(),
        )),
        Some(days) if days > MAX_STATISTICS_DAYS => Err(AppError::Validation(format!(
            "statistics days must not exceed {MAX_STATISTICS_DAYS}"
        ))),
        _ => Ok(days),
    }
}

pub async fn overview(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    days: Option<u32>,
) -> Result<Overview, AppError> {
    user.require("telemetry.read", None)?;
    let days = validate_days(days)?;
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
    let days = validate_days(days)?;
    Ok(
        stats_repo::application_stats(&installed.database, application_id, environment_id, days)
            .await?,
    )
}

#[cfg(test)]
mod tests {
    use super::validate_days;

    #[test]
    fn statistics_window_is_bounded() {
        assert!(validate_days(Some(0)).is_err());
        assert!(validate_days(Some(731)).is_err());
        assert_eq!(validate_days(Some(365)).ok().flatten(), Some(365));
    }
}
