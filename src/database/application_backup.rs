use std::collections::HashMap;

use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr, QueryResult,
    sea_query::{Alias, Expr, ExprTrait, Order, Query, Value},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{
    applications,
    backup_models::{
        self, BackupAlertRule, BackupApiKey, BackupApplication, BackupAuditLog,
        BackupDailyAggregate, BackupEnvironment, BackupEvent, BackupLog, BackupNotificationChannel,
        BackupRole, BackupRoleBinding, BackupUser, ExportedApiKey, ExportedApplication,
        ExportedEnvironment, ExportedEvent, ExportedLog,
    },
    query::{insert_batch, insert_batch_ignore_conflicts},
};

pub const FORMAT_VERSION: &str = "1.1";
const LEGACY_FORMAT_VERSION: &str = "1.0";
const APPLICATION_EXPORT_TYPE: &str = "sonde_application";
const SYSTEM_BACKUP_TYPE: &str = "sonde_full_backup";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistogramBackup {
    pub count: u64,
    pub sum: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    #[serde(default)]
    pub explicit_bounds: Vec<f64>,
    #[serde(default)]
    pub bucket_counts: Vec<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedMetricPoint {
    pub environment_id: String,
    pub name: String,
    pub metric_type: String,
    pub value: f64,
    pub unit: Option<String>,
    pub timestamp: i64,
    pub attributes: serde_json::Value,
    pub received_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub histogram: Option<HistogramBackup>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupMetricPoint {
    pub id: String,
    pub application_id: String,
    pub environment_id: String,
    pub name: String,
    pub metric_type: String,
    pub value: f64,
    pub unit: Option<String>,
    pub timestamp: i64,
    pub attributes: String,
    pub received_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub histogram: Option<HistogramBackup>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedTelemetry {
    pub events: Vec<ExportedEvent>,
    pub metric_points: Vec<ExportedMetricPoint>,
    pub logs: Vec<ExportedLog>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SingleAppExport {
    pub format_version: String,
    pub export_type: String,
    pub exported_at: i64,
    pub application: ExportedApplication,
    pub environments: Vec<ExportedEnvironment>,
    pub api_keys: Vec<ExportedApiKey>,
    pub telemetry: ExportedTelemetry,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FullSystemBackup {
    pub format_version: String,
    pub backup_type: String,
    pub exported_at: i64,
    pub server_version: String,
    pub users: Vec<BackupUser>,
    pub roles: Vec<BackupRole>,
    pub role_bindings: Vec<BackupRoleBinding>,
    pub applications: Vec<BackupApplication>,
    pub environments: Vec<BackupEnvironment>,
    pub api_keys: Vec<BackupApiKey>,
    pub alert_rules: Vec<BackupAlertRule>,
    pub notification_channels: Vec<BackupNotificationChannel>,
    pub audit_log: Vec<BackupAuditLog>,
    pub events: Vec<BackupEvent>,
    pub metric_points: Vec<BackupMetricPoint>,
    pub logs: Vec<BackupLog>,
    pub daily_aggregates: Vec<BackupDailyAggregate>,
}

pub async fn export_single_application(
    database: &DatabaseConnection,
    application_id: &str,
) -> Result<Option<SingleAppExport>, DbErr> {
    let Some(base) = backup_models::export_single_application(database, application_id).await?
    else {
        return Ok(None);
    };
    let metrics = export_application_metrics(database, application_id).await?;
    let backup_models::SingleAppExport {
        export_type,
        exported_at,
        application,
        environments,
        api_keys,
        telemetry,
        ..
    } = base;

    Ok(Some(SingleAppExport {
        format_version: FORMAT_VERSION.into(),
        export_type,
        exported_at,
        application,
        environments,
        api_keys,
        telemetry: ExportedTelemetry {
            events: telemetry.events,
            metric_points: metrics,
            logs: telemetry.logs,
        },
    }))
}

pub async fn import_single_application(
    database: &DatabaseConnection,
    owner_user_id: Option<&str>,
    payload: SingleAppExport,
) -> Result<String, DbErr> {
    validate_format(
        &payload.format_version,
        &payload.export_type,
        APPLICATION_EXPORT_TYPE,
    )?;

    let environment_slugs = payload
        .environments
        .iter()
        .map(|environment| (environment.id.clone(), environment.slug.clone()))
        .collect::<HashMap<_, _>>();
    let metrics = payload.telemetry.metric_points;
    let legacy = backup_models::SingleAppExport {
        format_version: LEGACY_FORMAT_VERSION.into(),
        export_type: payload.export_type,
        exported_at: payload.exported_at,
        application: payload.application,
        environments: payload.environments,
        api_keys: payload.api_keys,
        telemetry: backup_models::ExportedTelemetry {
            events: payload.telemetry.events,
            metric_points: Vec::new(),
            logs: payload.telemetry.logs,
        },
    };

    let new_application_id =
        backup_models::import_single_application(database, owner_user_id, legacy).await?;
    if metrics.is_empty() {
        return Ok(new_application_id);
    }

    let imported_environments = applications::list_environments(database, &new_application_id).await?;
    let by_slug = imported_environments
        .iter()
        .map(|environment| (environment.slug.as_str(), environment.id.as_str()))
        .collect::<HashMap<_, _>>();
    let fallback_environment = imported_environments
        .first()
        .map(|environment| environment.id.as_str())
        .ok_or_else(|| DbErr::Custom("imported application has no environment".into()))?;

    let mut rows = Vec::with_capacity(metrics.len());
    for metric in metrics {
        let mapped_environment = environment_slugs
            .get(&metric.environment_id)
            .and_then(|slug| by_slug.get(slug.as_str()).copied())
            .unwrap_or(fallback_environment);
        rows.push(application_metric_row(
            &new_application_id,
            mapped_environment,
            metric,
        )?);
    }
    insert_batch(database, "metric_points", metric_columns(), rows).await?;
    Ok(new_application_id)
}

pub async fn export_full_system(database: &DatabaseConnection) -> Result<FullSystemBackup, DbErr> {
    let base = backup_models::export_full_system(database).await?;
    let metrics = export_system_metrics(database).await?;
    let backup_models::FullSystemBackup {
        backup_type,
        exported_at,
        server_version,
        users,
        roles,
        role_bindings,
        applications,
        environments,
        api_keys,
        alert_rules,
        notification_channels,
        audit_log,
        events,
        logs,
        daily_aggregates,
        ..
    } = base;

    Ok(FullSystemBackup {
        format_version: FORMAT_VERSION.into(),
        backup_type,
        exported_at,
        server_version,
        users,
        roles,
        role_bindings,
        applications,
        environments,
        api_keys,
        alert_rules,
        notification_channels,
        audit_log,
        events,
        metric_points: metrics,
        logs,
        daily_aggregates,
    })
}

pub async fn restore_full_system(
    database: &DatabaseConnection,
    payload: FullSystemBackup,
) -> Result<(), DbErr> {
    validate_format(
        &payload.format_version,
        &payload.backup_type,
        SYSTEM_BACKUP_TYPE,
    )?;
    let metrics = payload.metric_points;
    let legacy = backup_models::FullSystemBackup {
        format_version: LEGACY_FORMAT_VERSION.into(),
        backup_type: payload.backup_type,
        exported_at: payload.exported_at,
        server_version: payload.server_version,
        users: payload.users,
        roles: payload.roles,
        role_bindings: payload.role_bindings,
        applications: payload.applications,
        environments: payload.environments,
        api_keys: payload.api_keys,
        alert_rules: payload.alert_rules,
        notification_channels: payload.notification_channels,
        audit_log: payload.audit_log,
        events: payload.events,
        metric_points: Vec::new(),
        logs: payload.logs,
        daily_aggregates: payload.daily_aggregates,
    };
    backup_models::restore_full_system(database, legacy).await?;

    if metrics.is_empty() {
        return Ok(());
    }
    let mut rows = Vec::with_capacity(metrics.len());
    for metric in metrics {
        rows.push(system_metric_row(metric)?);
    }
    insert_batch_ignore_conflicts(
        database,
        "metric_points",
        metric_columns(),
        rows,
        "id",
        "id",
    )
    .await?;
    Ok(())
}

async fn export_application_metrics(
    database: &DatabaseConnection,
    application_id: &str,
) -> Result<Vec<ExportedMetricPoint>, DbErr> {
    let query = Query::select()
        .columns(
            application_metric_select_columns()
                .iter()
                .map(|column| Alias::new(*column)),
        )
        .from(Alias::new("metric_points"))
        .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
        .order_by(Alias::new("timestamp"), Order::Asc)
        .order_by(Alias::new("id"), Order::Asc)
        .to_owned();
    database
        .query_all(&query)
        .await?
        .into_iter()
        .map(map_application_metric)
        .collect()
}

async fn export_system_metrics(
    database: &DatabaseConnection,
) -> Result<Vec<BackupMetricPoint>, DbErr> {
    let query = Query::select()
        .columns(metric_columns().iter().map(|column| Alias::new(*column)))
        .from(Alias::new("metric_points"))
        .order_by(Alias::new("id"), Order::Asc)
        .to_owned();
    database
        .query_all(&query)
        .await?
        .into_iter()
        .map(map_system_metric)
        .collect()
}

fn map_application_metric(row: QueryResult) -> Result<ExportedMetricPoint, DbErr> {
    let attributes_raw: String = row.try_get("", "attributes")?;
    let attributes = serde_json::from_str(&attributes_raw)
        .map_err(|error| DbErr::Custom(format!("invalid metric attributes JSON: {error}")))?;
    Ok(ExportedMetricPoint {
        environment_id: row.try_get("", "environment_id")?,
        name: row.try_get("", "name")?,
        metric_type: row.try_get("", "metric_type")?,
        value: row.try_get("", "value")?,
        unit: row.try_get("", "unit")?,
        timestamp: row.try_get("", "timestamp")?,
        attributes,
        received_at: row.try_get("", "received_at")?,
        histogram: histogram_from_row(&row)?,
    })
}

fn map_system_metric(row: QueryResult) -> Result<BackupMetricPoint, DbErr> {
    Ok(BackupMetricPoint {
        id: row.try_get("", "id")?,
        application_id: row.try_get("", "application_id")?,
        environment_id: row.try_get("", "environment_id")?,
        name: row.try_get("", "name")?,
        metric_type: row.try_get("", "metric_type")?,
        value: row.try_get("", "value")?,
        unit: row.try_get("", "unit")?,
        timestamp: row.try_get("", "timestamp")?,
        attributes: row.try_get("", "attributes")?,
        received_at: row.try_get("", "received_at")?,
        histogram: histogram_from_row(&row)?,
    })
}

fn histogram_from_row(row: &QueryResult) -> Result<Option<HistogramBackup>, DbErr> {
    let Some(count) = row.try_get::<Option<i64>>("", "histogram_count")? else {
        return Ok(None);
    };
    let count = u64::try_from(count)
        .map_err(|_| DbErr::Custom("negative histogram count in database".into()))?;
    let bounds_raw = row
        .try_get::<Option<String>>("", "histogram_bounds")?
        .unwrap_or_else(|| "[]".into());
    let buckets_raw = row
        .try_get::<Option<String>>("", "histogram_bucket_counts")?
        .unwrap_or_else(|| "[]".into());
    let histogram = HistogramBackup {
        count,
        sum: row.try_get("", "histogram_sum")?,
        min: row.try_get("", "histogram_min")?,
        max: row.try_get("", "histogram_max")?,
        explicit_bounds: serde_json::from_str(&bounds_raw)
            .map_err(|error| DbErr::Custom(format!("invalid histogram bounds JSON: {error}")))?,
        bucket_counts: serde_json::from_str(&buckets_raw)
            .map_err(|error| DbErr::Custom(format!("invalid histogram bucket JSON: {error}")))?,
    };
    validate_histogram(&histogram)?;
    Ok(Some(histogram))
}

fn application_metric_row(
    application_id: &str,
    environment_id: &str,
    metric: ExportedMetricPoint,
) -> Result<Vec<Value>, DbErr> {
    metric_row(
        Uuid::now_v7().to_string(),
        application_id.to_owned(),
        environment_id.to_owned(),
        metric.name,
        metric.metric_type,
        metric.value,
        metric.unit,
        metric.timestamp,
        serde_json::to_string(&metric.attributes).map_err(|error| {
            DbErr::Custom(format!("metric attributes serialization failed: {error}"))
        })?,
        metric.received_at,
        metric.histogram,
    )
}

fn system_metric_row(metric: BackupMetricPoint) -> Result<Vec<Value>, DbErr> {
    metric_row(
        metric.id,
        metric.application_id,
        metric.environment_id,
        metric.name,
        metric.metric_type,
        metric.value,
        metric.unit,
        metric.timestamp,
        metric.attributes,
        metric.received_at,
        metric.histogram,
    )
}

#[allow(clippy::too_many_arguments)]
fn metric_row(
    id: String,
    application_id: String,
    environment_id: String,
    name: String,
    metric_type: String,
    value: f64,
    unit: Option<String>,
    timestamp: i64,
    attributes: String,
    received_at: i64,
    histogram: Option<HistogramBackup>,
) -> Result<Vec<Value>, DbErr> {
    if !value.is_finite() {
        return Err(DbErr::Custom("metric value must be finite".into()));
    }
    let histogram_values = histogram_values(histogram.as_ref())?;
    let mut row = vec![
        id.into(),
        application_id.into(),
        environment_id.into(),
        name.into(),
        metric_type.into(),
        value.into(),
        unit.map(Into::into).unwrap_or(Value::String(None)),
        timestamp.into(),
        attributes.into(),
        received_at.into(),
    ];
    row.extend(histogram_values);
    Ok(row)
}

fn histogram_values(histogram: Option<&HistogramBackup>) -> Result<Vec<Value>, DbErr> {
    let Some(histogram) = histogram else {
        return Ok(vec![
            Value::BigInt(None),
            Value::Double(None),
            Value::Double(None),
            Value::Double(None),
            Value::String(None),
            Value::String(None),
        ]);
    };
    validate_histogram(histogram)?;
    let count = i64::try_from(histogram.count)
        .map_err(|_| DbErr::Custom("histogram count exceeds database range".into()))?;
    Ok(vec![
        count.into(),
        histogram.sum.map(Into::into).unwrap_or(Value::Double(None)),
        histogram.min.map(Into::into).unwrap_or(Value::Double(None)),
        histogram.max.map(Into::into).unwrap_or(Value::Double(None)),
        serde_json::to_string(&histogram.explicit_bounds)
            .map_err(|error| DbErr::Custom(error.to_string()))?
            .into(),
        serde_json::to_string(&histogram.bucket_counts)
            .map_err(|error| DbErr::Custom(error.to_string()))?
            .into(),
    ])
}

fn validate_histogram(histogram: &HistogramBackup) -> Result<(), DbErr> {
    for value in histogram
        .explicit_bounds
        .iter()
        .copied()
        .chain(histogram.sum)
        .chain(histogram.min)
        .chain(histogram.max)
    {
        if !value.is_finite() {
            return Err(DbErr::Custom("histogram contains a non-finite value".into()));
        }
    }
    if histogram
        .explicit_bounds
        .windows(2)
        .any(|window| window[0] >= window[1])
    {
        return Err(DbErr::Custom(
            "histogram bounds must be strictly increasing".into(),
        ));
    }
    if histogram.bucket_counts.len() != histogram.explicit_bounds.len().saturating_add(1) {
        return Err(DbErr::Custom(
            "histogram bucket count length must equal bounds length plus one".into(),
        ));
    }
    let bucket_total = histogram
        .bucket_counts
        .iter()
        .try_fold(0_u64, |total, count| total.checked_add(*count))
        .ok_or_else(|| DbErr::Custom("histogram bucket counts overflow".into()))?;
    if bucket_total != histogram.count {
        return Err(DbErr::Custom(
            "histogram bucket counts must sum to histogram count".into(),
        ));
    }
    if histogram
        .min
        .zip(histogram.max)
        .is_some_and(|(min, max)| min > max)
    {
        return Err(DbErr::Custom("histogram min must not exceed max".into()));
    }
    Ok(())
}

fn validate_format(version: &str, actual_type: &str, expected_type: &str) -> Result<(), DbErr> {
    if !matches!(version, FORMAT_VERSION | LEGACY_FORMAT_VERSION) {
        return Err(DbErr::Custom(format!(
            "unsupported backup format version {version}"
        )));
    }
    if actual_type != expected_type {
        return Err(DbErr::Custom(format!(
            "unexpected backup type {actual_type}"
        )));
    }
    Ok(())
}

fn application_metric_select_columns() -> &'static [&'static str] {
    &[
        "id",
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
    ]
}

fn metric_columns() -> &'static [&'static str] {
    &[
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
    ]
}

#[cfg(test)]
mod tests {
    use super::{HistogramBackup, validate_histogram};

    #[test]
    fn histogram_backup_validation_rejects_inconsistent_population() {
        let invalid = HistogramBackup {
            count: 3,
            sum: Some(4.0),
            min: Some(1.0),
            max: Some(2.0),
            explicit_bounds: vec![1.5],
            bucket_counts: vec![1, 1],
        };
        assert!(validate_histogram(&invalid).is_err());
    }

    #[test]
    fn histogram_backup_validation_accepts_valid_population() {
        let valid = HistogramBackup {
            count: 3,
            sum: Some(4.0),
            min: Some(1.0),
            max: Some(2.0),
            explicit_bounds: vec![1.5],
            bucket_counts: vec![1, 2],
        };
        assert!(validate_histogram(&valid).is_ok());
    }
}
