use sea_orm::{DatabaseConnection, DbErr, TransactionTrait};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::domain::telemetry::{
    Attributes, ErrorInput, ErrorSeverity, EventInput, HistogramInput, LogInput, LogLevel,
    MetricInput, MetricType,
};

use super::{
    log_error_rollup,
    query::{insert_batch, insert_batch_ignore_conflicts},
    rollups,
};

#[derive(Clone, Debug)]
pub struct TelemetryScope {
    pub application_id: String,
    pub environment_id: String,
}

pub async fn insert_events(
    database: &DatabaseConnection,
    scope: &TelemetryScope,
    events: impl IntoIterator<Item = EventInput>,
) -> Result<usize, DbErr> {
    let mut events: Vec<EventInput> = events.into_iter().collect();
    if events.is_empty() {
        return Ok(0);
    }
    let transaction = database.begin().await?;
    let received_at = chrono::Utc::now().timestamp_millis();
    let columns = [
        "id",
        "application_id",
        "environment_id",
        "name",
        "timestamp",
        "day",
        "anonymous_id",
        "session_id",
        "app_version",
        "launcher_version",
        "os",
        "attributes",
        "dedupe_key",
        "received_at",
    ];
    let mut inserted = 0_u64;
    let mut day_cache = UtcDayCache::default();
    let dirty_timestamps: Vec<i64> = events
        .iter()
        .map(|event| event.timestamp.unwrap_or(received_at))
        .collect();

    for chunk in events.chunks_mut(100) {
        let mut rows = Vec::with_capacity(chunk.len());
        for event in chunk {
            let timestamp = event.timestamp.unwrap_or(received_at);
            let day = day_cache.get_day(timestamp);
            let attributes_json = encode_attributes(&event.attributes);
            let dedupe_key = event
                .idempotency_key
                .as_deref()
                .map(|key| scoped_event_dedupe_key(scope, key));
            rows.push(vec![
                Uuid::now_v7().to_string().into(),
                scope.application_id.clone().into(),
                scope.environment_id.clone().into(),
                std::mem::take(&mut event.name).into_string().into(),
                timestamp.into(),
                day.into(),
                event.anonymous_id.take().map(|s| s.into_string()).into(),
                event.session_id.take().map(|s| s.into_string()).into(),
                event.app_version.take().map(|s| s.into_string()).into(),
                event
                    .launcher_version
                    .take()
                    .map(|s| s.into_string())
                    .into(),
                event.os.take().map(|s| s.into_string()).into(),
                attributes_json.into(),
                dedupe_key.into(),
                received_at.into(),
            ]);
        }
        inserted += insert_batch_ignore_conflicts(
            &transaction,
            "events",
            &columns,
            rows,
            "dedupe_key",
            "id",
        )
        .await?;
    }
    if inserted > 0 {
        rollups::mark_dirty_timestamps_for_source(
            &transaction,
            scope,
            rollups::DIRTY_SOURCE_EVENT,
            dirty_timestamps,
        )
        .await?;
    }
    transaction.commit().await?;
    Ok(inserted as usize)
}

pub async fn insert_metrics(
    database: &DatabaseConnection,
    scope: &TelemetryScope,
    metrics: impl IntoIterator<Item = MetricInput>,
) -> Result<usize, DbErr> {
    let mut metrics: Vec<MetricInput> = metrics.into_iter().collect();
    if metrics.is_empty() {
        return Ok(0);
    }
    let transaction = database.begin().await?;
    let received_at = chrono::Utc::now().timestamp_millis();
    let columns = [
        "id",
        "application_id",
        "environment_id",
        "name",
        "metric_type",
        "value",
        "unit",
        "timestamp",
        "attributes",
        "received_at",
        "histogram_count",
        "histogram_sum",
        "histogram_min",
        "histogram_max",
        "histogram_bounds",
        "histogram_bucket_counts",
    ];
    let dirty_timestamps: Vec<i64> = metrics
        .iter()
        .map(|metric| metric.timestamp.unwrap_or(received_at))
        .collect();

    let count = metrics.len();
    for chunk in metrics.chunks_mut(100) {
        let mut rows = Vec::with_capacity(chunk.len());
        for metric in chunk {
            let attributes_json = encode_attributes(&metric.attributes);
            let (
                histogram_count,
                histogram_sum,
                histogram_min,
                histogram_max,
                histogram_bounds,
                histogram_bucket_counts,
            ) = encode_histogram(metric)?;
            rows.push(vec![
                Uuid::now_v7().to_string().into(),
                scope.application_id.clone().into(),
                scope.environment_id.clone().into(),
                std::mem::take(&mut metric.name).into_string().into(),
                metric.metric_type.as_str().into(),
                metric.compatibility_value().into(),
                metric.unit.take().map(|s| s.into_string()).into(),
                metric.timestamp.unwrap_or(received_at).into(),
                attributes_json.into(),
                received_at.into(),
                histogram_count.into(),
                histogram_sum.into(),
                histogram_min.into(),
                histogram_max.into(),
                histogram_bounds.into(),
                histogram_bucket_counts.into(),
            ]);
        }
        insert_batch(&transaction, "metric_points", &columns, rows).await?;
    }
    rollups::mark_dirty_timestamps_for_source(
        &transaction,
        scope,
        rollups::DIRTY_SOURCE_METRIC,
        dirty_timestamps,
    )
    .await?;
    transaction.commit().await?;
    Ok(count)
}

type EncodedHistogram = (
    Option<i64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<String>,
    Option<String>,
);

fn encode_histogram(metric: &MetricInput) -> Result<EncodedHistogram, DbErr> {
    if metric.metric_type != MetricType::Histogram {
        return Ok((None, None, None, None, None, None));
    }
    if let Some(histogram) = metric.histogram.as_ref() {
        return encode_histogram_parts(histogram);
    }
    let Some(value) = metric.value else {
        return Ok((None, None, None, None, None, None));
    };
    Ok((
        Some(1),
        Some(value),
        Some(value),
        Some(value),
        Some(String::from("[]")),
        Some(String::from("[1]")),
    ))
}

fn encode_histogram_parts(histogram: &HistogramInput) -> Result<EncodedHistogram, DbErr> {
    let count = i64::try_from(histogram.count)
        .map_err(|_| DbErr::Custom("histogram count exceeds supported range".into()))?;
    let bounds = crate::json::encode_f64_array(&histogram.explicit_bounds).map_err(json_error)?;
    let bucket_counts =
        crate::json::encode_u64_array(&histogram.bucket_counts).map_err(json_error)?;
    Ok((
        Some(count),
        histogram.sum,
        histogram.min,
        histogram.max,
        Some(bounds),
        Some(bucket_counts),
    ))
}

pub async fn insert_logs(
    database: &DatabaseConnection,
    scope: &TelemetryScope,
    logs: impl IntoIterator<Item = LogInput>,
) -> Result<usize, DbErr> {
    let mut logs: Vec<LogInput> = logs.into_iter().collect();
    if logs.is_empty() {
        return Ok(0);
    }
    let transaction = database.begin().await?;
    let received_at = chrono::Utc::now().timestamp_millis();
    let columns = [
        "id",
        "application_id",
        "environment_id",
        "level",
        "message",
        "logger",
        "trace_id",
        "span_id",
        "timestamp",
        "attributes",
        "received_at",
    ];
    let dirty_timestamps: Vec<i64> = logs
        .iter()
        .map(|log| log.timestamp.unwrap_or(received_at))
        .collect();
    let error_timestamps: Vec<i64> = logs
        .iter()
        .filter(|log| matches!(&log.level, LogLevel::Error | LogLevel::Fatal))
        .map(|log| log.timestamp.unwrap_or(received_at))
        .collect();

    let count = logs.len();
    for chunk in logs.chunks_mut(100) {
        let mut rows = Vec::with_capacity(chunk.len());
        for log in chunk {
            let attributes_json = encode_attributes(&log.attributes);
            rows.push(vec![
                Uuid::now_v7().to_string().into(),
                scope.application_id.clone().into(),
                scope.environment_id.clone().into(),
                log.level.as_str().into(),
                std::mem::take(&mut log.message).into_string().into(),
                log.logger.take().map(|s| s.into_string()).into(),
                log.trace_id.take().map(|s| s.into_string()).into(),
                log.span_id.take().map(|s| s.into_string()).into(),
                log.timestamp.unwrap_or(received_at).into(),
                attributes_json.into(),
                received_at.into(),
            ]);
        }
        insert_batch(&transaction, "logs", &columns, rows).await?;
    }
    rollups::mark_dirty_timestamps_for_source(
        &transaction,
        scope,
        rollups::DIRTY_SOURCE_LOG,
        dirty_timestamps,
    )
    .await?;
    rollups::mark_dirty_timestamps_for_source(
        &transaction,
        scope,
        log_error_rollup::DIRTY_SOURCE_LOG_ERROR,
        error_timestamps,
    )
    .await?;
    transaction.commit().await?;
    Ok(count)
}

pub async fn insert_migrated_event(
    database: &DatabaseConnection,
    scope: &TelemetryScope,
    event: &EventInput,
    dedupe_key: &str,
) -> Result<bool, DbErr> {
    let transaction = database.begin().await?;
    let received_at = chrono::Utc::now().timestamp_millis();
    let timestamp = event.timestamp.unwrap_or(received_at);
    let day = utc_day(timestamp);
    let columns = [
        "id",
        "application_id",
        "environment_id",
        "name",
        "timestamp",
        "day",
        "anonymous_id",
        "session_id",
        "app_version",
        "launcher_version",
        "os",
        "attributes",
        "dedupe_key",
        "received_at",
    ];
    let rows = vec![vec![
        Uuid::now_v7().to_string().into(),
        scope.application_id.clone().into(),
        scope.environment_id.clone().into(),
        event.name.as_ref().into(),
        timestamp.into(),
        day.into(),
        event.anonymous_id.as_deref().into(),
        event.session_id.as_deref().into(),
        event.app_version.as_deref().into(),
        event.launcher_version.as_deref().into(),
        event.os.as_deref().into(),
        encode_attributes(&event.attributes).into(),
        dedupe_key.into(),
        received_at.into(),
    ]];
    let inserted =
        insert_batch_ignore_conflicts(&transaction, "events", &columns, rows, "dedupe_key", "id")
            .await?
            > 0;
    if inserted {
        rollups::mark_dirty_timestamps_for_source(
            &transaction,
            scope,
            rollups::DIRTY_SOURCE_EVENT,
            [timestamp],
        )
        .await?;
    }
    transaction.commit().await?;
    Ok(inserted)
}

fn encode_attributes(attributes: &Attributes) -> &str {
    attributes.as_str()
}

fn utc_day(timestamp: i64) -> String {
    chrono::DateTime::from_timestamp_millis(timestamp)
        .map(|value| value.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| String::from("1970-01-01"))
}

#[derive(Default)]
struct UtcDayCache {
    cached_day_epoch: Option<i64>,
    cached_str: String,
}

impl UtcDayCache {
    fn get_day(&mut self, timestamp_millis: i64) -> &str {
        let day_epoch = timestamp_millis.div_euclid(86_400_000);
        if self.cached_day_epoch != Some(day_epoch) {
            self.cached_str = utc_day(timestamp_millis);
            self.cached_day_epoch = Some(day_epoch);
        }
        &self.cached_str
    }
}

fn scoped_event_dedupe_key(scope: &TelemetryScope, idempotency_key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"sonde:event-idempotency:v1\0");
    hasher.update(scope.application_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(scope.environment_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(idempotency_key.as_bytes());
    format!("evt:{}", hex::encode(hasher.finalize()))
}

fn anonymous_hash(scope: &TelemetryScope, value: &str) -> String {
    super::device_identity::scoped_hash(scope, value)
}

fn json_error(error: serde_json::Error) -> DbErr {
    DbErr::Custom(error.to_string())
}

pub async fn insert_errors(
    database: &DatabaseConnection,
    scope: &TelemetryScope,
    errors: &[ErrorInput],
) -> Result<usize, DbErr> {
    if errors.is_empty() {
        return Ok(0);
    }
    let transaction = database.begin().await?;
    let received_at = chrono::Utc::now().timestamp_millis();
    let columns = [
        "id",
        "application_id",
        "environment_id",
        "level",
        "message",
        "logger",
        "trace_id",
        "span_id",
        "timestamp",
        "attributes",
        "received_at",
    ];

    for chunk in errors.chunks(100) {
        let mut rows = Vec::with_capacity(chunk.len());
        for err in chunk {
            let mut attrs = err.attributes.decoded().map_err(json_error)?;
            attrs.insert(
                "error_name".into(),
                serde_json::Value::from(err.name.as_ref()),
            );
            attrs.insert(
                "error_message".into(),
                serde_json::Value::from(err.message.as_ref()),
            );
            if let Some(ref st) = err.stack_trace {
                attrs.insert("stack_trace".into(), serde_json::Value::from(st.as_ref()));
            }
            if let Some(h) = err.handled {
                attrs.insert("handled".into(), serde_json::Value::Bool(h));
            }
            if let Some(ref anonymous_id) = err.anonymous_id {
                attrs.insert(
                    "anonymous_id".into(),
                    serde_json::Value::String(anonymous_hash(scope, anonymous_id)),
                );
            }
            if let Some(ref s) = err.session_id {
                attrs.insert("session_id".into(), serde_json::Value::from(s.as_ref()));
            }
            if let Some(ref v) = err.app_version {
                attrs.insert("app_version".into(), serde_json::Value::from(v.as_ref()));
            }
            if let Some(ref v) = err.launcher_version {
                attrs.insert(
                    "launcher_version".into(),
                    serde_json::Value::from(v.as_ref()),
                );
            }
            if let Some(ref os) = err.os {
                attrs.insert("os".into(), serde_json::Value::from(os.as_ref()));
            }
            let attributes_json = serde_json::to_string(&attrs).map_err(json_error)?;
            let level_str = match err.severity {
                Some(ErrorSeverity::Fatal) => "fatal",
                Some(ErrorSeverity::Warning) => "warn",
                _ => "error",
            };
            rows.push(vec![
                Uuid::now_v7().to_string().into(),
                scope.application_id.clone().into(),
                scope.environment_id.clone().into(),
                level_str.into(),
                format!("{}: {}", err.name, err.message).into(),
                Some("error_reporter".to_string()).into(),
                Option::<String>::None.into(),
                Option::<String>::None.into(),
                err.timestamp.unwrap_or(received_at).into(),
                attributes_json.into(),
                received_at.into(),
            ]);
        }
        insert_batch(&transaction, "logs", &columns, rows).await?;
    }

    super::errors::insert_error_index(&transaction, scope, errors, received_at).await?;
    rollups::mark_dirty_timestamps_for_source(
        &transaction,
        scope,
        rollups::DIRTY_SOURCE_LOG | rollups::DIRTY_SOURCE_ERROR,
        errors
            .iter()
            .map(|error| error.timestamp.unwrap_or(received_at)),
    )
    .await?;
    rollups::mark_dirty_timestamps_for_source(
        &transaction,
        scope,
        log_error_rollup::DIRTY_SOURCE_LOG_ERROR,
        errors
            .iter()
            .filter(|error| !matches!(error.severity.as_ref(), Some(ErrorSeverity::Warning)))
            .map(|error| error.timestamp.unwrap_or(received_at)),
    )
    .await?;
    transaction.commit().await?;
    Ok(errors.len())
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    use super::{TelemetryScope, UtcDayCache, anonymous_hash, scoped_event_dedupe_key, utc_day};

    #[test]
    fn idempotency_key_is_scoped_to_application_and_environment() {
        let a = TelemetryScope {
            application_id: "app-a".into(),
            environment_id: "prod".into(),
        };
        let b = TelemetryScope {
            application_id: "app-b".into(),
            environment_id: "prod".into(),
        };
        assert_ne!(
            scoped_event_dedupe_key(&a, "request-1"),
            scoped_event_dedupe_key(&b, "request-1")
        );
        assert_eq!(
            scoped_event_dedupe_key(&a, "request-1"),
            scoped_event_dedupe_key(&a, "request-1")
        );
    }

    #[test]
    fn error_anonymous_id_uses_the_event_compatibility_scope() {
        let scope = TelemetryScope {
            application_id: "app-a".into(),
            environment_id: "prod".into(),
        };
        let expected = {
            let mut hasher = Sha256::new();
            hasher.update(b"device-1");
            hasher.update(b"app-a:prod");
            hex::encode(hasher.finalize())
        };
        assert_eq!(anonymous_hash(&scope, "device-1"), expected);
    }

    #[test]
    fn utc_day_cache_matches_utc_day() {
        let mut cache = UtcDayCache::default();
        let timestamps = [
            0,
            1_000,
            86_399_999,
            86_400_000,
            1_700_000_000_000,
            1_700_000_001_000,
            -1_000,
            -86_400_000,
        ];
        for &ts in &timestamps {
            assert_eq!(cache.get_day(ts), &utc_day(ts));
        }
    }
}
