use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr, TransactionTrait,
    sea_query::{Alias, Expr, ExprTrait, Query},
};
use uuid::Uuid;

use crate::domain::telemetry::{EventInput, LogInput, MetricInput};

use super::query::{insert, insert_batch};

#[derive(Clone, Debug)]
pub struct TelemetryScope {
    pub application_id: String,
    pub environment_id: String,
}

pub async fn insert_events(
    database: &DatabaseConnection,
    scope: &TelemetryScope,
    events: &[EventInput],
) -> Result<usize, DbErr> {
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

    for chunk in events.chunks(100) {
        let mut rows = Vec::with_capacity(chunk.len());
        for event in chunk {
            let timestamp = event.timestamp.unwrap_or(received_at);
            let day = chrono::DateTime::from_timestamp_millis(timestamp)
                .map(|value| value.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| "1970-01-01".into());
            let attributes_json = if event.attributes.is_empty() {
                "{}".to_string()
            } else {
                serde_json::to_string(&event.attributes).map_err(json_error)?
            };
            rows.push(vec![
                Uuid::now_v7().to_string().into(),
                scope.application_id.clone().into(),
                scope.environment_id.clone().into(),
                event.name.clone().into(),
                timestamp.into(),
                day.into(),
                event.anonymous_id.clone().into(),
                event.session_id.clone().into(),
                event.app_version.clone().into(),
                event.launcher_version.clone().into(),
                event.os.clone().into(),
                attributes_json.into(),
                Option::<String>::None.into(),
                received_at.into(),
            ]);
        }
        insert_batch(&transaction, "events", &columns, rows).await?;
    }
    transaction.commit().await?;
    Ok(events.len())
}

pub async fn insert_metrics(
    database: &DatabaseConnection,
    scope: &TelemetryScope,
    metrics: &[MetricInput],
) -> Result<usize, DbErr> {
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
    ];

    for chunk in metrics.chunks(100) {
        let mut rows = Vec::with_capacity(chunk.len());
        for metric in chunk {
            let attributes_json = if metric.attributes.is_empty() {
                "{}".to_string()
            } else {
                serde_json::to_string(&metric.attributes).map_err(json_error)?
            };
            rows.push(vec![
                Uuid::now_v7().to_string().into(),
                scope.application_id.clone().into(),
                scope.environment_id.clone().into(),
                metric.name.clone().into(),
                format!("{:?}", metric.metric_type).to_lowercase().into(),
                metric.value.into(),
                metric.unit.clone().into(),
                metric.timestamp.unwrap_or(received_at).into(),
                attributes_json.into(),
                received_at.into(),
            ]);
        }
        insert_batch(&transaction, "metric_points", &columns, rows).await?;
    }
    transaction.commit().await?;
    Ok(metrics.len())
}

pub async fn insert_logs(
    database: &DatabaseConnection,
    scope: &TelemetryScope,
    logs: &[LogInput],
) -> Result<usize, DbErr> {
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

    for chunk in logs.chunks(100) {
        let mut rows = Vec::with_capacity(chunk.len());
        for log in chunk {
            let attributes_json = if log.attributes.is_empty() {
                "{}".to_string()
            } else {
                serde_json::to_string(&log.attributes).map_err(json_error)?
            };
            rows.push(vec![
                Uuid::now_v7().to_string().into(),
                scope.application_id.clone().into(),
                scope.environment_id.clone().into(),
                format!("{:?}", log.level).to_lowercase().into(),
                log.message.clone().into(),
                log.logger.clone().into(),
                log.trace_id.clone().into(),
                log.span_id.clone().into(),
                log.timestamp.unwrap_or(received_at).into(),
                attributes_json.into(),
                received_at.into(),
            ]);
        }
        insert_batch(&transaction, "logs", &columns, rows).await?;
    }
    transaction.commit().await?;
    Ok(logs.len())
}

pub async fn insert_migrated_event(
    database: &DatabaseConnection,
    scope: &TelemetryScope,
    event: &EventInput,
    dedupe_key: &str,
) -> Result<bool, DbErr> {
    let exists = Query::select()
        .expr(Expr::col(Alias::new("id")))
        .from(Alias::new("events"))
        .and_where(Expr::col(Alias::new("dedupe_key")).eq(dedupe_key))
        .limit(1)
        .to_owned();
    if database.query_one(&exists).await?.is_some() {
        return Ok(false);
    }
    let received_at = chrono::Utc::now().timestamp_millis();
    let timestamp = event.timestamp.unwrap_or(received_at);
    let day = chrono::DateTime::from_timestamp_millis(timestamp)
        .map(|value| value.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "1970-01-01".into());
    insert(
        database,
        "events",
        &[
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
        ],
        vec![
            Uuid::now_v7().to_string().into(),
            scope.application_id.clone().into(),
            scope.environment_id.clone().into(),
            event.name.clone().into(),
            timestamp.into(),
            day.into(),
            event.anonymous_id.clone().into(),
            event.session_id.clone().into(),
            event.app_version.clone().into(),
            event.launcher_version.clone().into(),
            event.os.clone().into(),
            serde_json::to_string(&event.attributes)
                .map_err(json_error)?
                .into(),
            dedupe_key.into(),
            received_at.into(),
        ],
    )
    .await?;
    Ok(true)
}

fn json_error(error: serde_json::Error) -> DbErr {
    DbErr::Custom(error.to_string())
}

pub async fn insert_errors(
    database: &DatabaseConnection,
    scope: &TelemetryScope,
    errors: &[crate::domain::telemetry::ErrorInput],
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
            let mut attrs = err.attributes.clone();
            attrs.insert("error_name".into(), serde_json::Value::String(err.name.clone()));
            if let Some(ref st) = err.stack_trace {
                attrs.insert("stack_trace".into(), serde_json::Value::String(st.clone()));
            }
            if let Some(h) = err.handled {
                attrs.insert("handled".into(), serde_json::Value::Bool(h));
            }
            if let Some(ref s) = err.session_id {
                attrs.insert("session_id".into(), serde_json::Value::String(s.clone()));
            }
            if let Some(ref v) = err.app_version {
                attrs.insert("app_version".into(), serde_json::Value::String(v.clone()));
            }
            if let Some(ref os) = err.os {
                attrs.insert("os".into(), serde_json::Value::String(os.clone()));
            }
            let attributes_json = serde_json::to_string(&attrs).map_err(json_error)?;
            let level_str = match err.severity {
                Some(crate::domain::telemetry::ErrorSeverity::Fatal) => "fatal",
                Some(crate::domain::telemetry::ErrorSeverity::Warning) => "warn",
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
    transaction.commit().await?;
    Ok(errors.len())
}
