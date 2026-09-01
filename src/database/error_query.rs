use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr,
    sea_query::{Alias, Expr, ExprTrait, Order, Query},
};
use serde::Serialize;

#[derive(Clone, Debug)]
pub struct ErrorGroupFilter {
    pub application_id: String,
    pub environment_id: Option<String>,
    pub severity: Option<String>,
    pub from: Option<i64>,
    pub to: Option<i64>,
    pub page: u64,
    pub page_size: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorGroupRecord {
    pub id: String,
    pub application_id: String,
    pub environment_id: String,
    pub fingerprint: String,
    pub name: String,
    pub message_sample: String,
    pub severity: String,
    pub first_seen: i64,
    pub last_seen: i64,
    pub occurrences: u64,
    pub last_app_version: Option<String>,
    pub last_launcher_version: Option<String>,
    pub last_os: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorGroupPage {
    pub items: Vec<ErrorGroupRecord>,
    pub page: u64,
    pub page_size: u64,
    pub has_more: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorOccurrenceRecord {
    pub id: String,
    pub group_id: String,
    pub timestamp: i64,
    pub anonymous_id: Option<String>,
    pub session_id: Option<String>,
    pub app_version: Option<String>,
    pub launcher_version: Option<String>,
    pub os: Option<String>,
    pub stack_trace: Option<String>,
    pub handled: Option<bool>,
    pub attributes: serde_json::Value,
    pub received_at: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorOccurrencePage {
    pub items: Vec<ErrorOccurrenceRecord>,
    pub page: u64,
    pub page_size: u64,
    pub has_more: bool,
}

pub async fn groups(
    database: &DatabaseConnection,
    filter: &ErrorGroupFilter,
) -> Result<ErrorGroupPage, DbErr> {
    let mut query = Query::select();
    query
        .columns(
            [
                "id",
                "application_id",
                "environment_id",
                "fingerprint",
                "name",
                "message_sample",
                "severity",
                "first_seen",
                "last_seen",
                "occurrences",
                "last_app_version",
                "last_launcher_version",
                "last_os",
            ]
            .map(Alias::new),
        )
        .from(Alias::new("error_groups"))
        .and_where(Expr::col(Alias::new("application_id")).eq(&filter.application_id));
    if let Some(environment_id) = filter.environment_id.as_deref() {
        query.and_where(Expr::col(Alias::new("environment_id")).eq(environment_id));
    }
    if let Some(severity) = filter.severity.as_deref() {
        query.and_where(Expr::col(Alias::new("severity")).eq(severity));
    }
    if let Some(from) = filter.from {
        query.and_where(Expr::col(Alias::new("last_seen")).gte(from));
    }
    if let Some(to) = filter.to {
        query.and_where(Expr::col(Alias::new("last_seen")).lte(to));
    }

    let offset = filter.page.saturating_sub(1).saturating_mul(filter.page_size);
    query
        .order_by(Alias::new("last_seen"), Order::Desc)
        .limit(filter.page_size.saturating_add(1))
        .offset(offset);

    let rows = database.query_all(&query).await?;
    let has_more = rows.len() > filter.page_size as usize;
    let mut items = Vec::with_capacity(std::cmp::min(
        rows.len(),
        filter.page_size as usize,
    ));
    for row in rows.into_iter().take(filter.page_size as usize) {
        let occurrences = u64::try_from(row.try_get::<i64>("", "occurrences")?).unwrap_or(0);
        items.push(ErrorGroupRecord {
            id: row.try_get("", "id")?,
            application_id: row.try_get("", "application_id")?,
            environment_id: row.try_get("", "environment_id")?,
            fingerprint: row.try_get("", "fingerprint")?,
            name: row.try_get("", "name")?,
            message_sample: row.try_get("", "message_sample")?,
            severity: row.try_get("", "severity")?,
            first_seen: row.try_get("", "first_seen")?,
            last_seen: row.try_get("", "last_seen")?,
            occurrences,
            last_app_version: optional_string(&row, "last_app_version")?,
            last_launcher_version: optional_string(&row, "last_launcher_version")?,
            last_os: optional_string(&row, "last_os")?,
        });
    }

    Ok(ErrorGroupPage {
        items,
        page: filter.page,
        page_size: filter.page_size,
        has_more,
    })
}

pub async fn group(
    database: &DatabaseConnection,
    group_id: &str,
) -> Result<Option<ErrorGroupRecord>, DbErr> {
    let query = Query::select()
        .columns(
            [
                "id",
                "application_id",
                "environment_id",
                "fingerprint",
                "name",
                "message_sample",
                "severity",
                "first_seen",
                "last_seen",
                "occurrences",
                "last_app_version",
                "last_launcher_version",
                "last_os",
            ]
            .map(Alias::new),
        )
        .from(Alias::new("error_groups"))
        .and_where(Expr::col(Alias::new("id")).eq(group_id))
        .limit(1)
        .to_owned();
    let Some(row) = database.query_one(&query).await? else {
        return Ok(None);
    };
    Ok(Some(ErrorGroupRecord {
        id: row.try_get("", "id")?,
        application_id: row.try_get("", "application_id")?,
        environment_id: row.try_get("", "environment_id")?,
        fingerprint: row.try_get("", "fingerprint")?,
        name: row.try_get("", "name")?,
        message_sample: row.try_get("", "message_sample")?,
        severity: row.try_get("", "severity")?,
        first_seen: row.try_get("", "first_seen")?,
        last_seen: row.try_get("", "last_seen")?,
        occurrences: u64::try_from(row.try_get::<i64>("", "occurrences")?).unwrap_or(0),
        last_app_version: optional_string(&row, "last_app_version")?,
        last_launcher_version: optional_string(&row, "last_launcher_version")?,
        last_os: optional_string(&row, "last_os")?,
    }))
}

pub async fn occurrences(
    database: &DatabaseConnection,
    group_id: &str,
    page: u64,
    page_size: u64,
    from: Option<i64>,
    to: Option<i64>,
) -> Result<ErrorOccurrencePage, DbErr> {
    let mut query = Query::select();
    query
        .columns(
            [
                "id",
                "group_id",
                "timestamp",
                "anonymous_id",
                "session_id",
                "app_version",
                "launcher_version",
                "os",
                "stack_trace",
                "handled",
                "attributes",
                "received_at",
            ]
            .map(Alias::new),
        )
        .from(Alias::new("error_occurrences"))
        .and_where(Expr::col(Alias::new("group_id")).eq(group_id));
    if let Some(from) = from {
        query.and_where(Expr::col(Alias::new("timestamp")).gte(from));
    }
    if let Some(to) = to {
        query.and_where(Expr::col(Alias::new("timestamp")).lte(to));
    }
    let offset = page.saturating_sub(1).saturating_mul(page_size);
    query
        .order_by(Alias::new("timestamp"), Order::Desc)
        .limit(page_size.saturating_add(1))
        .offset(offset);

    let rows = database.query_all(&query).await?;
    let has_more = rows.len() > page_size as usize;
    let mut items = Vec::with_capacity(std::cmp::min(rows.len(), page_size as usize));
    for row in rows.into_iter().take(page_size as usize) {
        let attributes: String = row.try_get("", "attributes")?;
        let handled = row
            .try_get::<Option<i64>>("", "handled")?
            .map(|value| value != 0);
        items.push(ErrorOccurrenceRecord {
            id: row.try_get("", "id")?,
            group_id: row.try_get("", "group_id")?,
            timestamp: row.try_get("", "timestamp")?,
            anonymous_id: optional_string(&row, "anonymous_id")?,
            session_id: optional_string(&row, "session_id")?,
            app_version: optional_string(&row, "app_version")?,
            launcher_version: optional_string(&row, "launcher_version")?,
            os: optional_string(&row, "os")?,
            stack_trace: optional_string(&row, "stack_trace")?,
            handled,
            attributes: serde_json::from_str(&attributes)
                .map_err(|error| DbErr::Custom(error.to_string()))?,
            received_at: row.try_get("", "received_at")?,
        });
    }

    Ok(ErrorOccurrencePage {
        items,
        page,
        page_size,
        has_more,
    })
}

fn optional_string(
    row: &sea_orm::QueryResult,
    column: &str,
) -> Result<Option<String>, DbErr> {
    row.try_get("", column)
}
