use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr, QueryResult, TransactionTrait,
    sea_query::{Alias, Expr, ExprTrait, Query, Value},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::query::insert;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertRuleRecord {
    pub id: String,
    pub application_id: String,
    pub name: String,
    pub enabled: bool,
    pub source_kind: String,
    pub query: serde_json::Value,
    pub window_minutes: i32,
    pub cooldown_seconds: i32,
    pub last_state: String,
    pub last_evaluated_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationChannelRecord {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub config: serde_json::Value,
    pub enabled: bool,
    pub created_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertDeliveryRecord {
    pub id: String,
    pub rule_id: String,
    pub rule_name: Option<String>,
    pub channel_id: String,
    pub channel_name: Option<String>,
    pub status: String,
    pub attempts: i32,
    pub last_error: Option<String>,
    pub next_attempt_at: Option<i64>,
    pub created_at: i64,
}

pub struct NewRule<'a> {
    pub application_id: &'a str,
    pub name: &'a str,
    pub source_kind: &'a str,
    pub query: &'a serde_json::Value,
    pub window_minutes: i32,
    pub cooldown_seconds: i32,
}

pub struct UpdateRule<'a> {
    pub name: &'a str,
    pub enabled: bool,
    pub source_kind: &'a str,
    pub query: &'a serde_json::Value,
    pub window_minutes: i32,
    pub cooldown_seconds: i32,
}

pub async fn create_rule(
    database: &DatabaseConnection,
    rule: NewRule<'_>,
) -> Result<String, DbErr> {
    let id = Uuid::now_v7().to_string();
    insert(
        database,
        "alert_rules",
        &[
            "id",
            "application_id",
            "name",
            "enabled",
            "source_kind",
            "query_json",
            "window_minutes",
            "cooldown_seconds",
            "last_state",
            "last_evaluated_at",
            "created_at",
        ],
        vec![
            id.clone().into(),
            rule.application_id.into(),
            rule.name.into(),
            true.into(),
            rule.source_kind.into(),
            rule.query.to_string().into(),
            rule.window_minutes.into(),
            rule.cooldown_seconds.into(),
            "healthy".into(),
            Option::<i64>::None.into(),
            chrono::Utc::now().timestamp_millis().into(),
        ],
    )
    .await?;
    Ok(id)
}

pub async fn list_rules(
    database: &DatabaseConnection,
    application_id: Option<&str>,
) -> Result<Vec<AlertRuleRecord>, DbErr> {
    let mut query = Query::select();
    query
        .columns(
            [
                "id",
                "application_id",
                "name",
                "enabled",
                "source_kind",
                "query_json",
                "window_minutes",
                "cooldown_seconds",
                "last_state",
                "last_evaluated_at",
            ]
            .map(Alias::new),
        )
        .from(Alias::new("alert_rules"))
        .order_by(Alias::new("created_at"), sea_orm::Order::Desc);
    if let Some(application_id) = application_id {
        query.and_where(Expr::col(Alias::new("application_id")).eq(application_id));
    }
    database
        .query_all(&query)
        .await?
        .into_iter()
        .map(map_rule)
        .collect()
}

pub async fn get_rule(
    database: &DatabaseConnection,
    id: &str,
) -> Result<Option<AlertRuleRecord>, DbErr> {
    let query = Query::select()
        .columns(
            [
                "id",
                "application_id",
                "name",
                "enabled",
                "source_kind",
                "query_json",
                "window_minutes",
                "cooldown_seconds",
                "last_state",
                "last_evaluated_at",
            ]
            .map(Alias::new),
        )
        .from(Alias::new("alert_rules"))
        .and_where(Expr::col(Alias::new("id")).eq(id))
        .limit(1)
        .to_owned();
    database
        .query_one(&query)
        .await?
        .map(map_rule)
        .transpose()
}

pub async fn update_rule(
    database: &DatabaseConnection,
    id: &str,
    rule: UpdateRule<'_>,
) -> Result<(), DbErr> {
    let transaction = database.begin().await?;
    let update = Query::update()
        .table(Alias::new("alert_rules"))
        .value(Alias::new("name"), rule.name)
        .value(Alias::new("enabled"), rule.enabled)
        .value(Alias::new("source_kind"), rule.source_kind)
        .value(Alias::new("query_json"), rule.query.to_string())
        .value(Alias::new("window_minutes"), rule.window_minutes)
        .value(Alias::new("cooldown_seconds"), rule.cooldown_seconds)
        .and_where(Expr::col(Alias::new("id")).eq(id))
        .to_owned();
    transaction.execute(&update).await?;
    if !rule.enabled {
        cancel_pending_deliveries(
            &transaction,
            "rule_id",
            id,
            "alert rule was disabled before delivery",
        )
        .await?;
    }
    transaction.commit().await
}

pub async fn update_rule_state(
    database: &DatabaseConnection,
    id: &str,
    last_state: &str,
    last_evaluated_at: i64,
) -> Result<(), DbErr> {
    let update = Query::update()
        .table(Alias::new("alert_rules"))
        .value(Alias::new("last_state"), last_state)
        .value(Alias::new("last_evaluated_at"), last_evaluated_at)
        .and_where(Expr::col(Alias::new("id")).eq(id))
        .to_owned();
    database.execute(&update).await?;
    Ok(())
}

pub async fn delete_rule(database: &DatabaseConnection, id: &str) -> Result<bool, DbErr> {
    let transaction = database.begin().await?;
    cancel_pending_deliveries(
        &transaction,
        "rule_id",
        id,
        "alert rule was deleted before delivery",
    )
    .await?;
    let delete = Query::delete()
        .from_table(Alias::new("alert_rules"))
        .and_where(Expr::col(Alias::new("id")).eq(id))
        .to_owned();
    let deleted = transaction.execute(&delete).await?.rows_affected() > 0;
    transaction.commit().await?;
    Ok(deleted)
}

// ----------------------------------------------------
// Notification Channels
// ----------------------------------------------------

pub async fn create_channel(
    database: &DatabaseConnection,
    name: &str,
    kind: &str,
    config: &serde_json::Value,
    enabled: bool,
) -> Result<String, DbErr> {
    let id = Uuid::now_v7().to_string();
    insert(
        database,
        "notification_channels",
        &["id", "name", "kind", "config_json", "enabled", "created_at"],
        vec![
            id.clone().into(),
            name.into(),
            kind.into(),
            config.to_string().into(),
            enabled.into(),
            chrono::Utc::now().timestamp_millis().into(),
        ],
    )
    .await?;
    Ok(id)
}

pub async fn list_channels(
    database: &DatabaseConnection,
) -> Result<Vec<NotificationChannelRecord>, DbErr> {
    let query = Query::select()
        .columns(["id", "name", "kind", "config_json", "enabled", "created_at"].map(Alias::new))
        .from(Alias::new("notification_channels"))
        .order_by(Alias::new("created_at"), sea_orm::Order::Asc)
        .to_owned();
    database
        .query_all(&query)
        .await?
        .into_iter()
        .map(map_channel)
        .collect()
}

pub async fn get_channel(
    database: &DatabaseConnection,
    id: &str,
) -> Result<Option<NotificationChannelRecord>, DbErr> {
    let query = Query::select()
        .columns(["id", "name", "kind", "config_json", "enabled", "created_at"].map(Alias::new))
        .from(Alias::new("notification_channels"))
        .and_where(Expr::col(Alias::new("id")).eq(id))
        .limit(1)
        .to_owned();
    database
        .query_one(&query)
        .await?
        .map(map_channel)
        .transpose()
}

pub async fn update_channel(
    database: &DatabaseConnection,
    id: &str,
    name: &str,
    kind: &str,
    config: &serde_json::Value,
    enabled: bool,
) -> Result<(), DbErr> {
    let transaction = database.begin().await?;
    let update = Query::update()
        .table(Alias::new("notification_channels"))
        .value(Alias::new("name"), name)
        .value(Alias::new("kind"), kind)
        .value(Alias::new("config_json"), config.to_string())
        .value(Alias::new("enabled"), enabled)
        .and_where(Expr::col(Alias::new("id")).eq(id))
        .to_owned();
    transaction.execute(&update).await?;
    if !enabled {
        cancel_pending_deliveries(
            &transaction,
            "channel_id",
            id,
            "notification channel was disabled before delivery",
        )
        .await?;
    }
    transaction.commit().await
}

pub async fn delete_channel(database: &DatabaseConnection, id: &str) -> Result<bool, DbErr> {
    let transaction = database.begin().await?;
    cancel_pending_deliveries(
        &transaction,
        "channel_id",
        id,
        "notification channel was deleted before delivery",
    )
    .await?;
    let delete = Query::delete()
        .from_table(Alias::new("notification_channels"))
        .and_where(Expr::col(Alias::new("id")).eq(id))
        .to_owned();
    let deleted = transaction.execute(&delete).await?.rows_affected() > 0;
    transaction.commit().await?;
    Ok(deleted)
}

// ----------------------------------------------------
// Alert Deliveries History
// ----------------------------------------------------

pub async fn list_deliveries(
    database: &DatabaseConnection,
    limit: u64,
) -> Result<Vec<AlertDeliveryRecord>, DbErr> {
    let query = Query::select()
        .columns(
            [
                (Alias::new("ad"), Alias::new("id")),
                (Alias::new("ad"), Alias::new("rule_id")),
                (Alias::new("ad"), Alias::new("channel_id")),
                (Alias::new("ad"), Alias::new("status")),
                (Alias::new("ad"), Alias::new("attempts")),
                (Alias::new("ad"), Alias::new("last_error")),
                (Alias::new("ad"), Alias::new("next_attempt_at")),
                (Alias::new("ad"), Alias::new("created_at")),
            ],
        )
        .expr_as(
            Expr::col((Alias::new("ar"), Alias::new("name"))),
            Alias::new("rule_name"),
        )
        .expr_as(
            Expr::col((Alias::new("nc"), Alias::new("name"))),
            Alias::new("channel_name"),
        )
        .from_as(Alias::new("alert_deliveries"), Alias::new("ad"))
        .left_join(
            Alias::new("alert_rules"),
            Expr::col((Alias::new("ad"), Alias::new("rule_id")))
                .equals((Alias::new("ar"), Alias::new("id"))),
        )
        .left_join(
            Alias::new("notification_channels"),
            Expr::col((Alias::new("ad"), Alias::new("channel_id")))
                .equals((Alias::new("nc"), Alias::new("id"))),
        )
        .order_by((Alias::new("ad"), Alias::new("created_at")), sea_orm::Order::Desc)
        .limit(limit)
        .to_owned();

    let rows = database.query_all(&query).await?;
    let mut result = Vec::with_capacity(rows.len());
    for row in rows {
        let attempts: i32 = row
            .try_get::<i32>("", "attempts")
            .unwrap_or_else(|_| row.try_get::<i64>("", "attempts").map(|v| v as i32).unwrap_or(1));
        result.push(AlertDeliveryRecord {
            id: row.try_get("", "id")?,
            rule_id: row.try_get("", "rule_id")?,
            rule_name: row.try_get("", "rule_name").ok(),
            channel_id: row.try_get("", "channel_id")?,
            channel_name: row.try_get("", "channel_name").ok(),
            status: row.try_get("", "status")?,
            attempts,
            last_error: row.try_get("", "last_error").ok(),
            next_attempt_at: row.try_get("", "next_attempt_at")?,
            created_at: row.try_get("", "created_at")?,
        });
    }
    Ok(result)
}

async fn cancel_pending_deliveries(
    database: &impl ConnectionTrait,
    foreign_key: &str,
    foreign_id: &str,
    reason: &str,
) -> Result<(), DbErr> {
    let update = Query::update()
        .table(Alias::new("alert_deliveries"))
        .value(Alias::new("status"), "cancelled")
        .value(Alias::new("last_error"), reason)
        .value(Alias::new("next_attempt_at"), Value::BigInt(None))
        .value(Alias::new("payload_json"), Value::String(None))
        .and_where(Expr::col(Alias::new(foreign_key)).eq(foreign_id))
        .and_where(Expr::col(Alias::new("status")).eq("pending"))
        .to_owned();
    database.execute(&update).await?;
    Ok(())
}

fn map_rule(row: QueryResult) -> Result<AlertRuleRecord, DbErr> {
    let query_json: String = row.try_get("", "query_json")?;
    let enabled = row
        .try_get::<bool>("", "enabled")
        .unwrap_or_else(|_| row.try_get::<i32>("", "enabled").map(|v| v == 1).unwrap_or(true));
    let window_minutes = row
        .try_get::<i32>("", "window_minutes")
        .unwrap_or_else(|_| row.try_get::<i64>("", "window_minutes").map(|v| v as i32).unwrap_or(5));
    let cooldown_seconds = row
        .try_get::<i32>("", "cooldown_seconds")
        .unwrap_or_else(|_| row.try_get::<i64>("", "cooldown_seconds").map(|v| v as i32).unwrap_or(300));
    Ok(AlertRuleRecord {
        id: row.try_get("", "id")?,
        application_id: row.try_get("", "application_id")?,
        name: row.try_get("", "name")?,
        enabled,
        source_kind: row.try_get("", "source_kind")?,
        query: serde_json::from_str(&query_json)
            .map_err(|error| DbErr::Custom(error.to_string()))?,
        window_minutes,
        cooldown_seconds,
        last_state: row.try_get("", "last_state")?,
        last_evaluated_at: row.try_get("", "last_evaluated_at").ok(),
    })
}

fn map_channel(row: QueryResult) -> Result<NotificationChannelRecord, DbErr> {
    let config_json: String = row.try_get("", "config_json")?;
    let enabled = row
        .try_get::<bool>("", "enabled")
        .unwrap_or_else(|_| row.try_get::<i32>("", "enabled").map(|v| v == 1).unwrap_or(true));
    Ok(NotificationChannelRecord {
        id: row.try_get("", "id")?,
        name: row.try_get("", "name")?,
        kind: row.try_get("", "kind")?,
        config: serde_json::from_str(&config_json)
            .map_err(|error| DbErr::Custom(error.to_string()))?,
        enabled,
        created_at: row.try_get("", "created_at")?,
    })
}
