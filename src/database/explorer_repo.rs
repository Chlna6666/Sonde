use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr, QueryResult,
    sea_query::{Alias, Expr, ExprTrait, Order, Query},
};
use serde::Serialize;
use serde_json::Value as JsonValue;

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
    pub launcher_version: Option<String>,
    pub os: Option<String>,
    pub attributes: JsonValue,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricRecord {
    pub id: String,
    pub name: String,
    pub metric_type: String,
    pub value: f64,
    pub unit: Option<String>,
    pub timestamp: i64,
    pub attributes: JsonValue,
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
    pub attributes: JsonValue,
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
                "launcher_version",
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
            launcher_version: row.try_get("", "launcher_version")?,
            os: row.try_get("", "os")?,
            attributes: json(row.try_get("", "attributes")?),
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
            unit: row.try_get("", "unit")?,
            timestamp: row.try_get("", "timestamp")?,
            attributes: json(row.try_get("", "attributes")?),
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
            attributes: json(row.try_get("", "attributes")?),
        })
    })
    .await
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
                .like(format!("%{}%", escape_like(value)))
                .or(Expr::col(Alias::new("attributes")).like(format!("%{}%", escape_like(value)))),
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
        .offset(filter.page.saturating_sub(1) * filter.page_size);
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

fn json(value: String) -> JsonValue {
    serde_json::from_str(&value).unwrap_or(JsonValue::Null)
}

fn escape_like(value: &str) -> String {
    value.replace('%', "\\%").replace('_', "\\_")
}
