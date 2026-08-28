use crate::{
    database::error_query_repo::{
        self, ErrorGroupFilter, ErrorGroupPage, ErrorGroupRecord, ErrorOccurrencePage,
    },
    error::AppError,
    services::authentication::AuthenticatedUser,
    state::InstalledState,
};

pub async fn groups(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    filter: &ErrorGroupFilter,
) -> Result<ErrorGroupPage, AppError> {
    user.require("telemetry.read", Some(&filter.application_id))?;
    Ok(error_query_repo::groups(&installed.database, filter).await?)
}

pub async fn group(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    group_id: &str,
) -> Result<ErrorGroupRecord, AppError> {
    let group = error_query_repo::group(&installed.database, group_id)
        .await?
        .ok_or(AppError::NotFound)?;
    user.require("telemetry.read", Some(&group.application_id))?;
    Ok(group)
}

pub async fn occurrences(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    group_id: &str,
    page: u64,
    page_size: u64,
    from: Option<i64>,
    to: Option<i64>,
) -> Result<ErrorOccurrencePage, AppError> {
    let group = error_query_repo::group(&installed.database, group_id)
        .await?
        .ok_or(AppError::NotFound)?;
    user.require("telemetry.read", Some(&group.application_id))?;
    Ok(error_query_repo::occurrences(
        &installed.database,
        group_id,
        page,
        page_size,
        from,
        to,
    )
    .await?)
}
