use crate::{
    database::explorer,
    error::AppError,
    services::authentication::AuthenticatedUser,
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
        launcher_version: record.launcher_version,
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
