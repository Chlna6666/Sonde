use crate::{
    database::explorer_repo::{self, EventRecord, ExplorerFilter, LogRecord, MetricRecord, Page},
    error::AppError,
    services::authentication::AuthenticatedUser,
    state::InstalledState,
};

pub async fn events(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    filter: &ExplorerFilter,
) -> Result<Page<EventRecord>, AppError> {
    authorize(user, filter)?;
    Ok(explorer_repo::events(&installed.database, filter).await?)
}

pub async fn metrics(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    filter: &ExplorerFilter,
) -> Result<Page<MetricRecord>, AppError> {
    authorize(user, filter)?;
    Ok(explorer_repo::metrics(&installed.database, filter).await?)
}

pub async fn logs(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    filter: &ExplorerFilter,
) -> Result<Page<LogRecord>, AppError> {
    authorize(user, filter)?;
    Ok(explorer_repo::logs(&installed.database, filter).await?)
}

fn authorize(user: &AuthenticatedUser, filter: &ExplorerFilter) -> Result<(), AppError> {
    user.require("telemetry.read", Some(&filter.application_id))
}
