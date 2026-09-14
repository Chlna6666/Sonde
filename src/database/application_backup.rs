use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr, QueryResult, TransactionTrait,
    sea_query::{Alias, Expr, ExprTrait, Order, Query, Value},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use super::application_transfer::{
    ExportedApiKey, ExportedApplication, ExportedEnvironment, ExportedEvent, ExportedLog,
};
use super::{
    application_transfer::{self, ApplicationTransfer},
    query::insert_batch,
};

pub const FORMAT_VERSION: &str = "1.1";
const APPLICATION_EXPORT_TYPE: &str = "sonde_application";

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

pub async fn export_single_application(
    database: &DatabaseConnection,
    application_id: &str,
) -> Result<Option<SingleAppExport>, DbErr> {
    let Some(base) = application_transfer::export_application(database, application_id).await?
    else {
        return Ok(None);
    };
    let metrics = export_application_metrics(database, application_id).await?;

    Ok(Some(SingleAppExport {
        format_version: FORMAT_VERSION.into(),
        export_type: APPLICATION_EXPORT_TYPE.into(),
        exported_at: base.exported_at,
        application: base.application,
        environments: base.environments,
        api_keys: base.api_keys,
        telemetry: ExportedTelemetry {
            events: base.events,
            metric_points: metrics,
            logs: base.logs,
        },
    }))
}

pub async fn import_single_application(
    database: &DatabaseConnection,
    owner_user_id: Option<&str>,
    payload: SingleAppExport,
) -> Result<String, DbErr> {
    validate_format(&payload.format_version, &payload.export_type)?;

    let SingleAppExport {
        exported_at,
        application,
        environments,
        api_keys,
        telemetry,
        ..
    } = payload;
    let ExportedTelemetry {
        events,
        metric_points,
        logs,
    } = telemetry;

    let transaction = database.begin().await?;
    let imported = application_transfer::import_application(
        &transaction,
        owner_user_id,
        ApplicationTransfer {
            exported_at,
            application,
            environments,
            api_keys,
            events,
            logs,
        },
    )
    .await?;

    if !metric_points.is_empty() {
        let mut rows = Vec::with_capacity(metric_points.len());
        for metric in metric_points {
            let environment_id = imported
                .environment_ids
                .get(&metric.environment_id)
                .unwrap_or(&imported.fallback_environment_id);
            rows.push(application_metric_row(
                &imported.id,
                environment_id,
                metric,
            )?);
        }
        insert_batch(&transaction, "metric_points", metric_columns(), rows).await?;
    }

    transaction.commit().await?;
    Ok(imported.id)
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
    if !metric.value.is_finite() {
        return Err(DbErr::Custom("metric value must be finite".into()));
    }
    let histogram_values = histogram_values(metric.histogram.as_ref())?;
    let mut row = vec![
        Uuid::now_v7().to_string().into(),
        application_id.to_owned().into(),
        environment_id.to_owned().into(),
        metric.name.into(),
        metric.metric_type.into(),
        metric.value.into(),
        Value::from(metric.unit),
        metric.timestamp.into(),
        serde_json::to_string(&metric.attributes)
            .map_err(|error| {
                DbErr::Custom(format!("metric attributes serialization failed: {error}"))
            })?
            .into(),
        metric.received_at.into(),
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
        Value::from(histogram.sum),
        Value::from(histogram.min),
        Value::from(histogram.max),
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
            return Err(DbErr::Custom(
                "histogram contains a non-finite value".into(),
            ));
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

fn validate_format(version: &str, export_type: &str) -> Result<(), DbErr> {
    if version != FORMAT_VERSION {
        return Err(DbErr::Custom(format!(
            "unsupported application backup format version {version}"
        )));
    }
    if export_type != APPLICATION_EXPORT_TYPE {
        return Err(DbErr::Custom(format!(
            "unexpected application backup type {export_type}"
        )));
    }
    Ok(())
}

fn application_metric_select_columns() -> &'static [&'static str] {
    &[
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
