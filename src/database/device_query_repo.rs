use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr,
    sea_query::{Alias, Condition, Expr, ExprTrait, Func, Order, Query},
};

#[derive(Clone, Debug)]
pub struct DeviceProfileFilter<'a> {
    pub application_id: &'a str,
    pub environment_id: Option<&'a str>,
    pub last_seen_from: Option<i64>,
    pub last_seen_to: Option<i64>,
    pub min_risk: Option<i32>,
    pub max_risk: Option<i32>,
    pub search: Option<&'a str>,
    pub page: u64,
    pub page_size: u64,
}

#[derive(Clone, Debug)]
pub struct DeviceProfileRecord {
    pub id: String,
    pub environment_id: String,
    pub last_seen_at: i64,
    pub last_event_at: Option<i64>,
    pub last_metric_at: Option<i64>,
    pub last_log_at: Option<i64>,
    pub last_error_at: Option<i64>,
    pub last_session_id: Option<String>,
    pub last_app_version: Option<String>,
    pub last_launcher_version: Option<String>,
    pub last_os: Option<String>,
    pub event_items: i64,
    pub metric_items: i64,
    pub log_items: i64,
    pub error_items: i64,
    pub session_changes: i64,
    pub app_version_changes: i64,
    pub launcher_version_changes: i64,
    pub os_changes: i64,
    pub risk_score: i32,
    pub last_anomaly: Option<String>,
    pub last_anomaly_at: Option<i64>,
}

#[derive(Clone, Debug)]
pub struct DeviceProfilePageRecord {
    pub items: Vec<DeviceProfileRecord>,
    pub total: u64,
}

#[derive(Clone, Debug)]
pub struct DeviceSecuritySummaryRecord {
    pub total: u64,
    pub active: u64,
    pub recent: u64,
    pub offline: u64,
    pub high_risk: u64,
    pub critical: u64,
}

pub async fn list_profiles(
    database: &DatabaseConnection,
    filter: &DeviceProfileFilter<'_>,
) -> Result<DeviceProfilePageRecord, DbErr> {
    let condition = build_condition(filter);
    let total = count_with_condition(database, condition.clone()).await?;

    let query = Query::select()
        .columns(
            [
                "id",
                "environment_id",
                "last_seen_at",
                "last_event_at",
                "last_metric_at",
                "last_log_at",
                "last_error_at",
                "last_session_id",
                "last_app_version",
                "last_launcher_version",
                "last_os",
                "event_items",
                "metric_items",
                "log_items",
                "error_items",
                "session_changes",
                "app_version_changes",
                "launcher_version_changes",
                "os_changes",
                "risk_score",
                "last_anomaly",
                "last_anomaly_at",
            ]
            .map(Alias::new),
        )
        .from(Alias::new("telemetry_devices"))
        .cond_where(condition)
        .order_by(Alias::new("risk_score"), Order::Desc)
        .order_by(Alias::new("last_seen_at"), Order::Desc)
        .limit(filter.page_size.max(1))
        .offset(filter.page.saturating_sub(1).saturating_mul(filter.page_size.max(1)))
        .to_owned();

    let items = database
        .query_all(&query)
        .await?
        .into_iter()
        .map(|row| {
            Ok(DeviceProfileRecord {
                id: row.try_get("", "id")?,
                environment_id: row.try_get("", "environment_id")?,
                last_seen_at: row.try_get("", "last_seen_at")?,
                last_event_at: row.try_get("", "last_event_at")?,
                last_metric_at: row.try_get("", "last_metric_at")?,
                last_log_at: row.try_get("", "last_log_at")?,
                last_error_at: row.try_get("", "last_error_at")?,
                last_session_id: row.try_get("", "last_session_id")?,
                last_app_version: row.try_get("", "last_app_version")?,
                last_launcher_version: row.try_get("", "last_launcher_version")?,
                last_os: row.try_get("", "last_os")?,
                event_items: row.try_get("", "event_items")?,
                metric_items: row.try_get("", "metric_items")?,
                log_items: row.try_get("", "log_items")?,
                error_items: row.try_get("", "error_items")?,
                session_changes: row.try_get("", "session_changes")?,
                app_version_changes: row.try_get("", "app_version_changes")?,
                launcher_version_changes: row.try_get("", "launcher_version_changes")?,
                os_changes: row.try_get("", "os_changes")?,
                risk_score: row.try_get("", "risk_score")?,
                last_anomaly: row.try_get("", "last_anomaly")?,
                last_anomaly_at: row.try_get("", "last_anomaly_at")?,
            })
        })
        .collect::<Result<Vec<_>, DbErr>>()?;

    Ok(DeviceProfilePageRecord { items, total })
}

pub async fn security_summary(
    database: &DatabaseConnection,
    application_id: &str,
    environment_id: Option<&str>,
    active_since: i64,
    recent_since: i64,
) -> Result<DeviceSecuritySummaryRecord, DbErr> {
    let base = scope_condition(application_id, environment_id);
    let total = count_with_condition(database, base.clone()).await?;
    let active = count_with_condition(
        database,
        base.clone().add(Expr::col(Alias::new("last_seen_at")).gte(active_since)),
    )
    .await?;
    let recent = count_with_condition(
        database,
        base.clone()
            .add(Expr::col(Alias::new("last_seen_at")).gte(recent_since))
            .add(Expr::col(Alias::new("last_seen_at")).lt(active_since)),
    )
    .await?;
    let offline = count_with_condition(
        database,
        base.clone().add(Expr::col(Alias::new("last_seen_at")).lt(recent_since)),
    )
    .await?;
    let high_risk = count_with_condition(
        database,
        base.clone().add(Expr::col(Alias::new("risk_score")).gte(50)),
    )
    .await?;
    let critical = count_with_condition(
        database,
        base.add(Expr::col(Alias::new("risk_score")).gte(80)),
    )
    .await?;

    Ok(DeviceSecuritySummaryRecord {
        total,
        active,
        recent,
        offline,
        high_risk,
        critical,
    })
}

fn build_condition(filter: &DeviceProfileFilter<'_>) -> Condition {
    let mut condition = scope_condition(filter.application_id, filter.environment_id);
    if let Some(value) = filter.last_seen_from {
        condition = condition.add(Expr::col(Alias::new("last_seen_at")).gte(value));
    }
    if let Some(value) = filter.last_seen_to {
        condition = condition.add(Expr::col(Alias::new("last_seen_at")).lt(value));
    }
    if let Some(value) = filter.min_risk {
        condition = condition.add(Expr::col(Alias::new("risk_score")).gte(value));
    }
    if let Some(value) = filter.max_risk {
        condition = condition.add(Expr::col(Alias::new("risk_score")).lte(value));
    }
    if let Some(search) = filter.search.filter(|value| !value.is_empty()) {
        let pattern = format!("%{search}%");
        condition = condition.add(
            Condition::any()
                .add(Expr::col(Alias::new("device_hash")).like(pattern.clone()))
                .add(Expr::col(Alias::new("last_session_id")).like(pattern.clone()))
                .add(Expr::col(Alias::new("last_app_version")).like(pattern.clone()))
                .add(Expr::col(Alias::new("last_launcher_version")).like(pattern.clone()))
                .add(Expr::col(Alias::new("last_os")).like(pattern)),
        );
    }
    condition
}

fn scope_condition(application_id: &str, environment_id: Option<&str>) -> Condition {
    let mut condition = Condition::all()
        .add(Expr::col(Alias::new("application_id")).eq(application_id));
    if let Some(environment_id) = environment_id {
        condition = condition.add(Expr::col(Alias::new("environment_id")).eq(environment_id));
    }
    condition
}

async fn count_with_condition(
    database: &impl ConnectionTrait,
    condition: Condition,
) -> Result<u64, DbErr> {
    let query = Query::select()
        .expr_as(Func::count(Expr::col(Alias::new("id"))), Alias::new("count"))
        .from(Alias::new("telemetry_devices"))
        .cond_where(condition)
        .to_owned();
    let count = database
        .query_one(&query)
        .await?
        .map(|row| row.try_get::<i64>("", "count"))
        .transpose()?
        .unwrap_or(0);
    u64::try_from(count).map_err(|_| DbErr::Custom("negative device profile count".into()))
}
