use crate::{
    database::explorer, error::AppError, services::authentication::AuthenticatedUser,
    state::InstalledState,
};

pub use super::explorer_models::{
    EventRecord, ExplorerFilter, HistogramRecord, LogRecord, MetricRecord, Page,
};

pub async fn events(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    filter: &ExplorerFilter,
) -> Result<Page<EventRecord>, AppError> {
    authorize(user, filter)?;
    let record = explorer::events(&installed.database, &database_filter(filter)).await?;
    Ok(map_page(record, map_event))
}

pub async fn metrics(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    filter: &ExplorerFilter,
) -> Result<Page<MetricRecord>, AppError> {
    authorize(user, filter)?;
    let record = explorer::metrics(&installed.database, &database_filter(filter)).await?;
    Ok(map_page(record, map_metric))
}

pub async fn logs(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    filter: &ExplorerFilter,
) -> Result<Page<LogRecord>, AppError> {
    authorize(user, filter)?;
    let record = explorer::logs(&installed.database, &database_filter(filter)).await?;
    Ok(map_page(record, map_log))
}

fn authorize(user: &AuthenticatedUser, filter: &ExplorerFilter) -> Result<(), AppError> {
    user.require("telemetry.read", Some(&filter.application_id))
}

fn database_filter(filter: &ExplorerFilter) -> explorer::ExplorerFilter {
    explorer::ExplorerFilter {
        application_id: filter.application_id.clone(),
        environment_id: filter.environment_id.clone(),
        from: filter.from,
        to: filter.to,
        name: filter.name.clone(),
        level: filter.level.clone(),
        text: filter.text.clone(),
        page: filter.page,
        page_size: filter.page_size,
    }
}

fn map_page<T, U>(record: explorer::Page<T>, map: fn(T) -> U) -> Page<U> {
    Page {
        items: record.items.into_iter().map(map).collect(),
        page: record.page,
        page_size: record.page_size,
        has_more: record.has_more,
    }
}

fn map_event(record: explorer::EventRecord) -> EventRecord {
    EventRecord {
        id: record.id,
        name: record.name,
        timestamp: record.timestamp,
        anonymous_id: record.anonymous_id,
        app_version: record.app_version,
        os: record.os,
        attributes: record.attributes,
    }
}

fn map_metric(record: explorer::MetricRecord) -> MetricRecord {
    MetricRecord {
        id: record.id,
        name: record.name,
        metric_type: record.metric_type,
        value: record.value,
        histogram: record.histogram.map(map_histogram),
        unit: record.unit,
        timestamp: record.timestamp,
        attributes: record.attributes,
    }
}

fn map_histogram(record: explorer::HistogramRecord) -> HistogramRecord {
    HistogramRecord {
        count: record.count,
        sum: record.sum,
        min: record.min,
        max: record.max,
        explicit_bounds: record.explicit_bounds,
        bucket_counts: record.bucket_counts,
    }
}

fn map_log(record: explorer::LogRecord) -> LogRecord {
    LogRecord {
        id: record.id,
        level: record.level,
        message: record.message,
        logger: record.logger,
        trace_id: record.trace_id,
        span_id: record.span_id,
        timestamp: record.timestamp,
        attributes: record.attributes,
    }
}

pub async fn delete_records(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    kind: &str,
    application_id: &str,
    environment_id: Option<&str>,
    ids: &[String],
) -> Result<u64, AppError> {
    user.require("apps.manage", Some(application_id))?;
    let application_id =
        crate::security::validate_safe_identifier("applicationId", application_id)?;
    let environment_id =
        crate::security::validate_optional_safe_identifier("environmentId", environment_id)?;
    let table = match kind {
        "events" => "events",
        "metrics" => "metric_points",
        "logs" => "logs",
        _ => return Err(AppError::Validation("unsupported explorer kind".into())),
    };
    if ids.is_empty() {
        return Ok(0);
    }
    if ids.len() > 1000 {
        return Err(AppError::Validation(
            "cannot delete more than 1000 records at once".into(),
        ));
    }
    let validated_ids: Vec<String> = ids
        .iter()
        .map(|id| crate::security::validate_safe_identifier("id", id))
        .collect::<Result<_, _>>()?;

    let count = explorer::delete_records(
        &installed.database,
        table,
        &application_id,
        environment_id.as_deref(),
        &validated_ids,
    )
    .await?;
    Ok(count)
}

pub async fn reset_records(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    kind: &str,
    application_id: &str,
    environment_id: Option<&str>,
) -> Result<u64, AppError> {
    user.require("apps.manage", Some(application_id))?;
    let application_id =
        crate::security::validate_safe_identifier("applicationId", application_id)?;
    let environment_id =
        crate::security::validate_optional_safe_identifier("environmentId", environment_id)?;
    let target = match kind {
        "events" => "events",
        "metrics" => "metric_points",
        "logs" => "logs",
        "all" => "all",
        _ => {
            return Err(AppError::Validation(
                "unsupported explorer reset kind".into(),
            ));
        }
    };

    let count = explorer::reset_records(
        &installed.database,
        target,
        &application_id,
        environment_id.as_deref(),
    )
    .await?;
    Ok(count)
}
