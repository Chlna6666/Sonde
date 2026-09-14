use crate::{
    database::error_query, error::AppError, services::authentication::AuthenticatedUser,
    state::InstalledState,
};

pub use super::error_query_models::{
    ErrorGroupFilter, ErrorGroupPage, ErrorGroupRecord, ErrorOccurrencePage, ErrorOccurrenceRecord,
};

pub async fn groups(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    filter: &ErrorGroupFilter,
) -> Result<ErrorGroupPage, AppError> {
    user.require("telemetry.read", Some(&filter.application_id))?;
    let record = error_query::groups(
        &installed.database,
        &error_query::ErrorGroupFilter {
            application_id: filter.application_id.clone(),
            environment_id: filter.environment_id.clone(),
            severity: filter.severity.clone(),
            from: filter.from,
            to: filter.to,
            page: filter.page,
            page_size: filter.page_size,
        },
    )
    .await?;
    Ok(map_group_page(record))
}

pub async fn group(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    group_id: &str,
) -> Result<ErrorGroupRecord, AppError> {
    let group = error_query::group(&installed.database, group_id)
        .await?
        .ok_or(AppError::NotFound)?;
    user.require("telemetry.read", Some(&group.application_id))?;
    Ok(map_group(group))
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
    let group = error_query::group(&installed.database, group_id)
        .await?
        .ok_or(AppError::NotFound)?;
    user.require("telemetry.read", Some(&group.application_id))?;
    let record =
        error_query::occurrences(&installed.database, group_id, page, page_size, from, to).await?;
    Ok(map_occurrence_page(record))
}

fn map_group_page(record: error_query::ErrorGroupPage) -> ErrorGroupPage {
    ErrorGroupPage {
        items: record.items.into_iter().map(map_group).collect(),
        page: record.page,
        page_size: record.page_size,
        has_more: record.has_more,
    }
}

fn map_group(record: error_query::ErrorGroupRecord) -> ErrorGroupRecord {
    ErrorGroupRecord {
        id: record.id,
        application_id: record.application_id,
        environment_id: record.environment_id,
        fingerprint: record.fingerprint,
        name: record.name,
        message_sample: record.message_sample,
        severity: record.severity,
        first_seen: record.first_seen,
        last_seen: record.last_seen,
        occurrences: record.occurrences,
        last_app_version: record.last_app_version,
        last_launcher_version: record.last_launcher_version,
        last_os: record.last_os,
    }
}

fn map_occurrence_page(record: error_query::ErrorOccurrencePage) -> ErrorOccurrencePage {
    ErrorOccurrencePage {
        items: record.items.into_iter().map(map_occurrence).collect(),
        page: record.page,
        page_size: record.page_size,
        has_more: record.has_more,
    }
}

fn map_occurrence(record: error_query::ErrorOccurrenceRecord) -> ErrorOccurrenceRecord {
    ErrorOccurrenceRecord {
        id: record.id,
        group_id: record.group_id,
        timestamp: record.timestamp,
        anonymous_id: record.anonymous_id,
        session_id: record.session_id,
        app_version: record.app_version,
        launcher_version: record.launcher_version,
        os: record.os,
        stack_trace: record.stack_trace,
        handled: record.handled,
        attributes: record.attributes,
        received_at: record.received_at,
    }
}
