use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr, QueryResult,
    sea_query::{Alias, Condition, Expr, ExprTrait, Func, Order, Query},
};
use serde::Serialize;
use serde_json::value::RawValue;

#[derive(Debug, Clone)]
pub struct ExplorerFilter {
    pub application_id: String,
    pub environment_id: Option<String>,
    pub from: Option<i64>,
    pub to: Option<i64>,
    pub name: Option<String>,
    pub level: Option<String>,
    pub text: Option<String>,
    pub page: u64,
    pub page_size: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    pub items: Vec<T>,
    pub page: u64,
    pub page_size: u64,
    pub has_more: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventRecord {
    pub id: String,
    pub name: String,
    pub timestamp: i64,
    pub anonymous_id: Option<String>,
    pub app_version: Option<String>,
    pub os: Option<String>,
    pub attributes: Box<RawValue>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricRecord {
    pub id: String,
    pub name: String,
    pub metric_type: String,
    /// Compatibility projection retained for existing Explorer consumers. Histogram-aware clients
    /// should read `histogram` so population weighting is not lost.
    pub value: f64,
    pub histogram: Option<HistogramRecord>,
    pub unit: Option<String>,
    pub timestamp: i64,
    pub attributes: Box<RawValue>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistogramRecord {
    pub count: u64,
    pub sum: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub explicit_bounds: Vec<f64>,
    pub bucket_counts: Vec<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogRecord {
    pub id: String,
    pub level: String,
    pub message: String,
    pub logger: Option<String>,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub timestamp: i64,
    pub attributes: Box<RawValue>,
}

pub async fn events(
    database: &DatabaseConnection,
    filter: &ExplorerFilter,
) -> Result<Page<EventRecord>, DbErr> {
    let mut select = Query::select();
    select
        .columns(
            [
                "id",
                "name",
                "timestamp",
                "anonymous_id",
                "app_version",
                "os",
                "attributes",
            ]
            .map(Alias::new),
        )
        .from(Alias::new("events"));
    apply_filter(&mut select, filter, "name", None);
    page(database, select, filter, |row| {
        Ok(EventRecord {
            id: row.try_get("", "id")?,
            name: row.try_get("", "name")?,
            timestamp: row.try_get("", "timestamp")?,
            anonymous_id: row.try_get("", "anonymous_id")?,
            app_version: row.try_get("", "app_version")?,
            os: row.try_get("", "os")?,
            attributes: decode_raw_json(row.try_get("", "attributes")?, "event attributes")?,
        })
    })
    .await
}

pub async fn metrics(
    database: &DatabaseConnection,
    filter: &ExplorerFilter,
) -> Result<Page<MetricRecord>, DbErr> {
    let mut select = Query::select();
    select
        .columns(
            [
                "id",
                "name",
                "metric_type",
                "value",
                "unit",
                "timestamp",
                "attributes",
                "histogram_count",
                "histogram_sum",
                "histogram_min",
                "histogram_max",
                "histogram_bounds",
                "histogram_bucket_counts",
            ]
            .map(Alias::new),
        )
        .from(Alias::new("metric_points"));
    apply_filter(&mut select, filter, "name", None);
    page(database, select, filter, |row| {
        Ok(MetricRecord {
            id: row.try_get("", "id")?,
            name: row.try_get("", "name")?,
            metric_type: row.try_get("", "metric_type")?,
            value: row.try_get("", "value")?,
            histogram: decode_histogram(&row)?,
            unit: row.try_get("", "unit")?,
            timestamp: row.try_get("", "timestamp")?,
            attributes: decode_raw_json(row.try_get("", "attributes")?, "metric attributes")?,
        })
    })
    .await
}

pub async fn logs(
    database: &DatabaseConnection,
    filter: &ExplorerFilter,
) -> Result<Page<LogRecord>, DbErr> {
    let mut select = Query::select();
    select
        .columns(
            [
                "id",
                "level",
                "message",
                "logger",
                "trace_id",
                "span_id",
                "timestamp",
                "attributes",
            ]
            .map(Alias::new),
        )
        .from(Alias::new("logs"));
    apply_filter(&mut select, filter, "message", Some("level"));
    page(database, select, filter, |row| {
        Ok(LogRecord {
            id: row.try_get("", "id")?,
            level: row.try_get("", "level")?,
            message: row.try_get("", "message")?,
            logger: row.try_get("", "logger")?,
            trace_id: row.try_get("", "trace_id")?,
            span_id: row.try_get("", "span_id")?,
            timestamp: row.try_get("", "timestamp")?,
            attributes: decode_raw_json(row.try_get("", "attributes")?, "log attributes")?,
        })
    })
    .await
}

fn decode_histogram(row: &QueryResult) -> Result<Option<HistogramRecord>, DbErr> {
    let Some(count) = row.try_get::<Option<i64>>("", "histogram_count")? else {
        return Ok(None);
    };
    let count = u64::try_from(count)
        .map_err(|_| DbErr::Custom("stored histogram count must not be negative".into()))?;
    let bounds = decode_json_vec::<f64>(
        row.try_get::<Option<String>>("", "histogram_bounds")?,
        "histogram_bounds",
    )?;
    let bucket_counts = decode_json_vec::<u64>(
        row.try_get::<Option<String>>("", "histogram_bucket_counts")?,
        "histogram_bucket_counts",
    )?;
    if !bucket_counts.is_empty() && bucket_counts.len() != bounds.len() + 1 {
        return Err(DbErr::Custom(
            "stored histogram bucket count length does not match bounds".into(),
        ));
    }
    let bucket_total = bucket_counts
        .iter()
        .try_fold(0_u64, |total, value| total.checked_add(*value))
        .ok_or_else(|| DbErr::Custom("stored histogram bucket counts overflow".into()))?;
    if !bucket_counts.is_empty() && bucket_total != count {
        return Err(DbErr::Custom(
            "stored histogram bucket counts do not sum to count".into(),
        ));
    }
    Ok(Some(HistogramRecord {
        count,
        sum: row.try_get("", "histogram_sum")?,
        min: row.try_get("", "histogram_min")?,
        max: row.try_get("", "histogram_max")?,
        explicit_bounds: bounds,
        bucket_counts,
    }))
}

fn decode_json_vec<T>(value: Option<String>, column: &str) -> Result<Vec<T>, DbErr>
where
    T: serde::de::DeserializeOwned,
{
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    serde_json::from_str(&value)
        .map_err(|error| DbErr::Custom(format!("invalid {column} JSON: {error}")))
}

fn decode_raw_json(value: String, field: &str) -> Result<Box<RawValue>, DbErr> {
    RawValue::from_string(value)
        .map_err(|error| DbErr::Custom(format!("invalid stored {field} JSON: {error}")))
}

fn apply_filter(
    select: &mut sea_orm::sea_query::SelectStatement,
    filter: &ExplorerFilter,
    searchable_column: &str,
    level_column: Option<&str>,
) {
    select.and_where(Expr::col(Alias::new("application_id")).eq(&filter.application_id));
    if let Some(value) = &filter.environment_id {
        select.and_where(Expr::col(Alias::new("environment_id")).eq(value));
    }
    if let Some(value) = filter.from {
        select.and_where(Expr::col(Alias::new("timestamp")).gte(value));
    }
    if let Some(value) = filter.to {
        select.and_where(Expr::col(Alias::new("timestamp")).lte(value));
    }
    if let Some(value) = &filter.name {
        select.and_where(Expr::col(Alias::new("name")).eq(value));
    }
    if let (Some(column), Some(value)) = (level_column, &filter.level) {
        select.and_where(Expr::col(Alias::new(column)).eq(value));
    }
    if let Some(value) = &filter.text {
        select.and_where(
            Expr::col(Alias::new(searchable_column))
                .like(crate::database::query::contains_like_pattern(value))
                .or(Expr::col(Alias::new("attributes"))
                    .like(crate::database::query::contains_like_pattern(value))),
        );
    }
}

async fn page<T, F>(
    database: &DatabaseConnection,
    mut select: sea_orm::sea_query::SelectStatement,
    filter: &ExplorerFilter,
    map: F,
) -> Result<Page<T>, DbErr>
where
    F: Fn(QueryResult) -> Result<T, DbErr>,
{
    select
        .order_by(Alias::new("timestamp"), Order::Desc)
        .limit(filter.page_size + 1)
        .offset(
            filter
                .page
                .saturating_sub(1)
                .saturating_mul(filter.page_size),
        );
    let mut items: Vec<T> = database
        .query_all(&select)
        .await?
        .into_iter()
        .map(map)
        .collect::<Result<_, _>>()?;
    let has_more = items.len() as u64 > filter.page_size;
    items.truncate(filter.page_size as usize);
    Ok(Page {
        items,
        page: filter.page,
        page_size: filter.page_size,
        has_more,
    })
}

const DELETE_ID_CHUNK: usize = 500;

pub async fn delete_records(
    database: &DatabaseConnection,
    table: &str,
    application_id: &str,
    environment_id: Option<&str>,
    ids: &[String],
) -> Result<u64, DbErr> {
    if ids.is_empty() {
        return Ok(0);
    }
    match table {
        "events" | "metric_points" | "logs" => {}
        _ => {
            return Err(DbErr::Custom(format!(
                "unsupported explorer table: {table}"
            )));
        }
    }

    let mut total_deleted: u64 = 0;
    for chunk in ids.chunks(DELETE_ID_CHUNK) {
        let mut delete = Query::delete();
        delete
            .from_table(Alias::new(table))
            .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
            .and_where(Expr::col(Alias::new("id")).is_in(chunk.iter().cloned()));
        if let Some(env_id) = environment_id {
            delete.and_where(Expr::col(Alias::new("environment_id")).eq(env_id));
        }
        let statement = delete.to_owned();
        let affected = database.execute(&statement).await?.rows_affected();
        total_deleted = total_deleted.saturating_add(affected);
    }

    Ok(total_deleted)
}

pub async fn reset_records(
    database: &DatabaseConnection,
    table_or_scope: &str,
    application_id: &str,
    environment_id: Option<&str>,
) -> Result<u64, DbErr> {
    let tables: &[&str] = match table_or_scope {
        "events" => &["events"],
        "metric_points" => &["metric_points"],
        "logs" => &["logs"],
        "all" => &["events", "metric_points", "logs"],
        _ => {
            return Err(DbErr::Custom(format!(
                "unsupported explorer reset scope: {table_or_scope}"
            )));
        }
    };

    let mut total_deleted: u64 = 0;
    for &table in tables {
        let mut delete = Query::delete();
        delete
            .from_table(Alias::new(table))
            .and_where(Expr::col(Alias::new("application_id")).eq(application_id));
        if let Some(env_id) = environment_id {
            delete.and_where(Expr::col(Alias::new("environment_id")).eq(env_id));
        }
        let statement = delete.to_owned();
        let affected = database.execute(&statement).await?.rows_affected();
        total_deleted = total_deleted.saturating_add(affected);
    }

    Ok(total_deleted)
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanInvalidResult {
    pub deleted_events: u64,
    pub deleted_dimensions: u64,
    pub deleted_metrics: u64,
    pub deleted_logs: u64,
    pub deleted_devices: u64,
    pub total_deleted: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InvalidRecordSample {
    pub kind: String,
    pub id: String,
    pub reason: String,
    pub detail: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanInvalidPreview {
    pub invalid_events: u64,
    pub invalid_dimensions: u64,
    pub invalid_metrics: u64,
    pub invalid_logs: u64,
    pub invalid_devices: u64,
    pub total_invalid: u64,
    pub samples: Vec<InvalidRecordSample>,
}

fn events_invalid_condition(application_id: Option<&str>) -> Condition {
    let mut cond = Condition::any()
        .add(Expr::col(Alias::new("os")).is_null())
        .add(Expr::col(Alias::new("os")).eq(""))
        .add(Expr::expr(Func::lower(Expr::col(Alias::new("os")))).eq("unknown"))
        .add(Expr::col(Alias::new("anonymous_id")).is_null())
        .add(Expr::col(Alias::new("anonymous_id")).eq(""))
        .add(Expr::expr(Func::lower(Expr::col(Alias::new("anonymous_id")))).eq("unknown"));
    if application_id.is_none() {
        let subquery = Query::select()
            .column(Alias::new("id"))
            .from(Alias::new("applications"))
            .to_owned();
        cond = cond.add(Expr::col(Alias::new("application_id")).not_in_subquery(subquery));
    }
    cond
}

fn dimensions_invalid_condition(application_id: Option<&str>) -> Condition {
    let mut cond = Condition::any()
        .add(Expr::col(Alias::new("dimension_value")).is_null())
        .add(Expr::col(Alias::new("dimension_value")).eq(""))
        .add(Expr::expr(Func::lower(Expr::col(Alias::new("dimension_value")))).eq("unknown"));
    if application_id.is_none() {
        let subquery = Query::select()
            .column(Alias::new("id"))
            .from(Alias::new("applications"))
            .to_owned();
        cond = cond.add(Expr::col(Alias::new("application_id")).not_in_subquery(subquery));
    }
    cond
}

fn metrics_invalid_condition(application_id: Option<&str>) -> Condition {
    let mut cond = Condition::any()
        .add(Expr::col(Alias::new("name")).is_null())
        .add(Expr::col(Alias::new("name")).eq(""));
    if application_id.is_none() {
        let subquery = Query::select()
            .column(Alias::new("id"))
            .from(Alias::new("applications"))
            .to_owned();
        cond = cond.add(Expr::col(Alias::new("application_id")).not_in_subquery(subquery));
    }
    cond
}

fn logs_invalid_condition(application_id: Option<&str>) -> Condition {
    let mut cond = Condition::any()
        .add(Expr::col(Alias::new("level")).is_null())
        .add(Expr::col(Alias::new("level")).eq(""));
    if application_id.is_none() {
        let subquery = Query::select()
            .column(Alias::new("id"))
            .from(Alias::new("applications"))
            .to_owned();
        cond = cond.add(Expr::col(Alias::new("application_id")).not_in_subquery(subquery));
    }
    cond
}

fn devices_invalid_condition(application_id: Option<&str>) -> Condition {
    let mut cond = Condition::any()
        .add(Expr::col(Alias::new("last_os")).is_null())
        .add(Expr::col(Alias::new("last_os")).eq(""))
        .add(Expr::expr(Func::lower(Expr::col(Alias::new("last_os")))).eq("unknown"))
        .add(Expr::col(Alias::new("id")).is_null())
        .add(Expr::col(Alias::new("id")).eq(""))
        .add(Expr::expr(Func::lower(Expr::col(Alias::new("id")))).eq("unknown"));
    if application_id.is_none() {
        let subquery = Query::select()
            .column(Alias::new("id"))
            .from(Alias::new("applications"))
            .to_owned();
        cond = cond.add(Expr::col(Alias::new("application_id")).not_in_subquery(subquery));
    }
    cond
}

pub async fn preview_clean_invalid_data(
    database: &DatabaseConnection,
    application_id: Option<&str>,
) -> Result<CleanInvalidPreview, DbErr> {
    let mut preview = CleanInvalidPreview::default();

    // 1. Preview invalid events
    {
        let cond = events_invalid_condition(application_id);
        let mut count_query = Query::select();
        count_query
            .expr_as(Func::count(Expr::col(Alias::new("id"))), Alias::new("cnt"))
            .from(Alias::new("events"))
            .cond_where(cond.clone());
        if let Some(app_id) = application_id {
            count_query.and_where(Expr::col(Alias::new("application_id")).eq(app_id));
        }
        let row = database.query_one(&count_query).await?;
        let count = row
            .and_then(|r| r.try_get::<i64>("", "cnt").ok())
            .unwrap_or(0);
        preview.invalid_events = u64::try_from(std::cmp::max(count, 0)).unwrap_or(0);

        if preview.invalid_events > 0 {
            let mut sample_query = Query::select();
            sample_query
                .columns(["id", "name", "os", "anonymous_id", "application_id"].map(Alias::new))
                .from(Alias::new("events"))
                .cond_where(cond);
            if let Some(app_id) = application_id {
                sample_query.and_where(Expr::col(Alias::new("application_id")).eq(app_id));
            }
            sample_query.limit(5);
            let rows = database.query_all(&sample_query).await?;
            for r in rows {
                let id: String = r.try_get("", "id").unwrap_or_default();
                let name: String = r.try_get("", "name").unwrap_or_default();
                let os: Option<String> = r.try_get("", "os").ok().flatten();
                let anon: Option<String> = r.try_get("", "anonymous_id").ok().flatten();
                let reason = if os.as_deref().unwrap_or("").trim().is_empty() {
                    "操作系统信息缺失".to_string()
                } else if os.as_deref().unwrap_or("").eq_ignore_ascii_case("unknown") {
                    "操作系统为 unknown".to_string()
                } else if anon.as_deref().unwrap_or("").trim().is_empty() {
                    "设备标识 (anonymousId) 缺失".to_string()
                } else if anon
                    .as_deref()
                    .unwrap_or("")
                    .eq_ignore_ascii_case("unknown")
                {
                    "设备标识为 unknown".to_string()
                } else {
                    "关联应用不存在 (孤立数据)".to_string()
                };
                let detail = format!("name: {}, os: {:?}, device: {:?}", name, os, anon);
                preview.samples.push(InvalidRecordSample {
                    kind: "events".to_string(),
                    id,
                    reason,
                    detail: Some(detail),
                });
            }
        }
    }

    // 2. Preview invalid dimensions
    {
        let cond = dimensions_invalid_condition(application_id);
        let mut count_query = Query::select();
        count_query
            .expr_as(Func::count(Expr::col(Alias::new("id"))), Alias::new("cnt"))
            .from(Alias::new("telemetry_daily_dimensions"))
            .cond_where(cond.clone());
        if let Some(app_id) = application_id {
            count_query.and_where(Expr::col(Alias::new("application_id")).eq(app_id));
        }
        let row = database.query_one(&count_query).await?;
        let count = row
            .and_then(|r| r.try_get::<i64>("", "cnt").ok())
            .unwrap_or(0);
        preview.invalid_dimensions = u64::try_from(std::cmp::max(count, 0)).unwrap_or(0);

        if preview.invalid_dimensions > 0 && preview.samples.len() < 10 {
            let mut sample_query = Query::select();
            sample_query
                .columns(["id", "dimension_key", "dimension_value", "day"].map(Alias::new))
                .from(Alias::new("telemetry_daily_dimensions"))
                .cond_where(cond);
            if let Some(app_id) = application_id {
                sample_query.and_where(Expr::col(Alias::new("application_id")).eq(app_id));
            }
            sample_query.limit(3);
            let rows = database.query_all(&sample_query).await?;
            for r in rows {
                let id: String = r.try_get("", "id").unwrap_or_default();
                let key: String = r.try_get("", "dimension_key").unwrap_or_default();
                let val: Option<String> = r.try_get("", "dimension_value").ok().flatten();
                let day: String = r.try_get("", "day").unwrap_or_default();
                preview.samples.push(InvalidRecordSample {
                    kind: "dimensions".to_string(),
                    id,
                    reason: "维度值为空或 unknown".to_string(),
                    detail: Some(format!("key: {}, value: {:?}, day: {}", key, val, day)),
                });
            }
        }
    }

    // 3. Preview invalid metrics
    {
        let cond = metrics_invalid_condition(application_id);
        let mut count_query = Query::select();
        count_query
            .expr_as(Func::count(Expr::col(Alias::new("id"))), Alias::new("cnt"))
            .from(Alias::new("metric_points"))
            .cond_where(cond.clone());
        if let Some(app_id) = application_id {
            count_query.and_where(Expr::col(Alias::new("application_id")).eq(app_id));
        }
        let row = database.query_one(&count_query).await?;
        let count = row
            .and_then(|r| r.try_get::<i64>("", "cnt").ok())
            .unwrap_or(0);
        preview.invalid_metrics = u64::try_from(std::cmp::max(count, 0)).unwrap_or(0);

        if preview.invalid_metrics > 0 && preview.samples.len() < 10 {
            let mut sample_query = Query::select();
            sample_query
                .columns(["id", "name"].map(Alias::new))
                .from(Alias::new("metric_points"))
                .cond_where(cond);
            if let Some(app_id) = application_id {
                sample_query.and_where(Expr::col(Alias::new("application_id")).eq(app_id));
            }
            sample_query.limit(3);
            let rows = database.query_all(&sample_query).await?;
            for r in rows {
                let id: String = r.try_get("", "id").unwrap_or_default();
                let name: Option<String> = r.try_get("", "name").ok().flatten();
                preview.samples.push(InvalidRecordSample {
                    kind: "metrics".to_string(),
                    id,
                    reason: "指标名称缺失或孤立".to_string(),
                    detail: Some(format!("name: {:?}", name)),
                });
            }
        }
    }

    // 4. Preview invalid logs
    {
        let cond = logs_invalid_condition(application_id);
        let mut count_query = Query::select();
        count_query
            .expr_as(Func::count(Expr::col(Alias::new("id"))), Alias::new("cnt"))
            .from(Alias::new("logs"))
            .cond_where(cond.clone());
        if let Some(app_id) = application_id {
            count_query.and_where(Expr::col(Alias::new("application_id")).eq(app_id));
        }
        let row = database.query_one(&count_query).await?;
        let count = row
            .and_then(|r| r.try_get::<i64>("", "cnt").ok())
            .unwrap_or(0);
        preview.invalid_logs = u64::try_from(std::cmp::max(count, 0)).unwrap_or(0);

        if preview.invalid_logs > 0 && preview.samples.len() < 10 {
            let mut sample_query = Query::select();
            sample_query
                .columns(["id", "level", "message"].map(Alias::new))
                .from(Alias::new("logs"))
                .cond_where(cond);
            if let Some(app_id) = application_id {
                sample_query.and_where(Expr::col(Alias::new("application_id")).eq(app_id));
            }
            sample_query.limit(3);
            let rows = database.query_all(&sample_query).await?;
            for r in rows {
                let id: String = r.try_get("", "id").unwrap_or_default();
                let level: Option<String> = r.try_get("", "level").ok().flatten();
                let message: String = r.try_get("", "message").unwrap_or_default();
                preview.samples.push(InvalidRecordSample {
                    kind: "logs".to_string(),
                    id,
                    reason: "日志级别缺失或孤立".to_string(),
                    detail: Some(format!("level: {:?}, msg: {}", level, message)),
                });
            }
        }
    }

    // 5. Preview invalid devices
    {
        let cond = devices_invalid_condition(application_id);
        let mut count_query = Query::select();
        count_query
            .expr_as(Func::count(Expr::col(Alias::new("id"))), Alias::new("cnt"))
            .from(Alias::new("telemetry_devices"))
            .cond_where(cond.clone());
        if let Some(app_id) = application_id {
            count_query.and_where(Expr::col(Alias::new("application_id")).eq(app_id));
        }
        let row = database.query_one(&count_query).await?;
        let count = row
            .and_then(|r| r.try_get::<i64>("", "cnt").ok())
            .unwrap_or(0);
        preview.invalid_devices = u64::try_from(std::cmp::max(count, 0)).unwrap_or(0);

        if preview.invalid_devices > 0 && preview.samples.len() < 10 {
            let mut sample_query = Query::select();
            sample_query
                .columns(["id", "last_os"].map(Alias::new))
                .from(Alias::new("telemetry_devices"))
                .cond_where(cond);
            if let Some(app_id) = application_id {
                sample_query.and_where(Expr::col(Alias::new("application_id")).eq(app_id));
            }
            sample_query.limit(3);
            let rows = database.query_all(&sample_query).await?;
            for r in rows {
                let id: String = r.try_get("", "id").unwrap_or_default();
                let last_os: Option<String> = r.try_get("", "last_os").ok().flatten();
                let reason = if last_os.as_deref().unwrap_or("").trim().is_empty() {
                    "设备操作系统信息缺失".to_string()
                } else if last_os
                    .as_deref()
                    .unwrap_or("")
                    .eq_ignore_ascii_case("unknown")
                {
                    "设备操作系统为 unknown".to_string()
                } else if id.eq_ignore_ascii_case("unknown") {
                    "设备 ID 为 unknown".to_string()
                } else {
                    "孤立设备记录".to_string()
                };
                preview.samples.push(InvalidRecordSample {
                    kind: "devices".to_string(),
                    id,
                    reason,
                    detail: Some(format!("last_os: {:?}", last_os)),
                });
            }
        }
    }

    preview.total_invalid = preview
        .invalid_events
        .saturating_add(preview.invalid_dimensions)
        .saturating_add(preview.invalid_metrics)
        .saturating_add(preview.invalid_logs)
        .saturating_add(preview.invalid_devices);

    Ok(preview)
}

pub async fn clean_invalid_data(
    database: &DatabaseConnection,
    application_id: Option<&str>,
) -> Result<CleanInvalidResult, DbErr> {
    let mut result = CleanInvalidResult::default();

    // 1. Delete invalid events
    {
        let mut delete = Query::delete();
        delete
            .from_table(Alias::new("events"))
            .cond_where(events_invalid_condition(application_id));
        if let Some(app_id) = application_id {
            delete.and_where(Expr::col(Alias::new("application_id")).eq(app_id));
        }
        let statement = delete.to_owned();
        result.deleted_events = database.execute(&statement).await?.rows_affected();
    }

    // 2. Delete invalid telemetry_daily_dimensions
    {
        let mut delete = Query::delete();
        delete
            .from_table(Alias::new("telemetry_daily_dimensions"))
            .cond_where(dimensions_invalid_condition(application_id));
        if let Some(app_id) = application_id {
            delete.and_where(Expr::col(Alias::new("application_id")).eq(app_id));
        }
        let statement = delete.to_owned();
        result.deleted_dimensions = database.execute(&statement).await?.rows_affected();
    }

    // 3. Delete invalid metric_points
    {
        let mut delete = Query::delete();
        delete
            .from_table(Alias::new("metric_points"))
            .cond_where(metrics_invalid_condition(application_id));
        if let Some(app_id) = application_id {
            delete.and_where(Expr::col(Alias::new("application_id")).eq(app_id));
        }
        let statement = delete.to_owned();
        result.deleted_metrics = database.execute(&statement).await?.rows_affected();
    }

    // 4. Delete invalid logs
    {
        let mut delete = Query::delete();
        delete
            .from_table(Alias::new("logs"))
            .cond_where(logs_invalid_condition(application_id));
        if let Some(app_id) = application_id {
            delete.and_where(Expr::col(Alias::new("application_id")).eq(app_id));
        }
        let statement = delete.to_owned();
        result.deleted_logs = database.execute(&statement).await?.rows_affected();
    }

    // 5. Delete invalid telemetry_devices
    {
        let mut delete = Query::delete();
        delete
            .from_table(Alias::new("telemetry_devices"))
            .cond_where(devices_invalid_condition(application_id));
        if let Some(app_id) = application_id {
            delete.and_where(Expr::col(Alias::new("application_id")).eq(app_id));
        }
        let statement = delete.to_owned();
        result.deleted_devices = database.execute(&statement).await?.rows_affected();
    }

    result.total_deleted = result
        .deleted_events
        .saturating_add(result.deleted_dimensions)
        .saturating_add(result.deleted_metrics)
        .saturating_add(result.deleted_logs)
        .saturating_add(result.deleted_devices);

    Ok(result)
}
