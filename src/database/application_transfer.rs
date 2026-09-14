use std::collections::HashMap;

use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr,
    sea_query::{Alias, Expr, ExprTrait, Order, Query, Value},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::query::insert;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedApplication {
    pub name: String,
    pub slug: String,
    pub retention_days: i32,
    pub is_public: bool,
    pub description: Option<String>,
    pub github_url: Option<String>,
    pub website_url: Option<String>,
    pub custom_header: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedEnvironment {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub created_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedApiKey {
    pub id: String,
    pub environment_id: String,
    pub name: String,
    pub key_hash: String,
    pub key_prefix: String,
    pub scopes: Vec<String>,
    pub expires_at: Option<i64>,
    pub last_used_at: Option<i64>,
    pub revoked_at: Option<i64>,
    pub created_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedEvent {
    pub environment_id: String,
    pub name: String,
    pub timestamp: i64,
    pub day: String,
    pub anonymous_id: Option<String>,
    pub session_id: Option<String>,
    pub app_version: Option<String>,
    pub launcher_version: Option<String>,
    pub os: Option<String>,
    pub attributes: serde_json::Value,
    pub dedupe_key: Option<String>,
    pub received_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedLog {
    pub environment_id: String,
    pub level: String,
    pub message: String,
    pub logger: Option<String>,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub timestamp: i64,
    pub attributes: serde_json::Value,
    pub received_at: i64,
}

pub(crate) struct ApplicationTransfer {
    pub exported_at: i64,
    pub application: ExportedApplication,
    pub environments: Vec<ExportedEnvironment>,
    pub api_keys: Vec<ExportedApiKey>,
    pub events: Vec<ExportedEvent>,
    pub logs: Vec<ExportedLog>,
}

pub(crate) struct ImportedApplication {
    pub id: String,
    pub environment_ids: HashMap<String, String>,
    pub fallback_environment_id: String,
}

pub(crate) async fn export_application(
    database: &DatabaseConnection,
    application_id: &str,
) -> Result<Option<ApplicationTransfer>, DbErr> {
    let app_query = Query::select()
        .columns(
            [
                "name",
                "slug",
                "retention_days",
                "is_public",
                "description",
                "github_url",
                "website_url",
                "custom_header",
                "created_at",
            ]
            .map(Alias::new),
        )
        .from(Alias::new("applications"))
        .and_where(Expr::col(Alias::new("id")).eq(application_id))
        .limit(1)
        .to_owned();
    let Some(app) = database.query_one(&app_query).await? else {
        return Ok(None);
    };
    let application = ExportedApplication {
        name: app.try_get("", "name")?,
        slug: app.try_get("", "slug")?,
        retention_days: app.try_get("", "retention_days")?,
        is_public: app.try_get("", "is_public").unwrap_or(false),
        description: app.try_get("", "description").ok(),
        github_url: app.try_get("", "github_url").ok(),
        website_url: app.try_get("", "website_url").ok(),
        custom_header: app.try_get("", "custom_header").ok(),
        created_at: app.try_get("", "created_at")?,
    };

    let environments = database
        .query_all(
            &Query::select()
                .columns(["id", "name", "slug", "created_at"].map(Alias::new))
                .from(Alias::new("environments"))
                .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
                .order_by(Alias::new("created_at"), Order::Asc)
                .to_owned(),
        )
        .await?
        .into_iter()
        .map(|row| {
            Ok(ExportedEnvironment {
                id: row.try_get("", "id")?,
                name: row.try_get("", "name")?,
                slug: row.try_get("", "slug")?,
                created_at: row.try_get("", "created_at")?,
            })
        })
        .collect::<Result<Vec<_>, DbErr>>()?;

    let api_keys = database
        .query_all(
            &Query::select()
                .columns(
                    [
                        "id",
                        "environment_id",
                        "name",
                        "key_hash",
                        "key_prefix",
                        "scopes",
                        "expires_at",
                        "last_used_at",
                        "revoked_at",
                        "created_at",
                    ]
                    .map(Alias::new),
                )
                .from(Alias::new("api_keys"))
                .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
                .to_owned(),
        )
        .await?
        .into_iter()
        .map(|row| {
            let scopes: String = row.try_get("", "scopes")?;
            Ok(ExportedApiKey {
                id: row.try_get("", "id")?,
                environment_id: row.try_get("", "environment_id")?,
                name: row.try_get("", "name")?,
                key_hash: row.try_get("", "key_hash")?,
                key_prefix: row.try_get("", "key_prefix")?,
                scopes: serde_json::from_str(&scopes).map_err(|error| {
                    DbErr::Custom(format!("invalid API key scopes JSON: {error}"))
                })?,
                expires_at: row.try_get("", "expires_at").ok(),
                last_used_at: row.try_get("", "last_used_at").ok(),
                revoked_at: row.try_get("", "revoked_at").ok(),
                created_at: row.try_get("", "created_at")?,
            })
        })
        .collect::<Result<Vec<_>, DbErr>>()?;

    let events = database
        .query_all(
            &Query::select()
                .columns(
                    [
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
                    ]
                    .map(Alias::new),
                )
                .from(Alias::new("events"))
                .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
                .order_by(Alias::new("timestamp"), Order::Asc)
                .order_by(Alias::new("id"), Order::Asc)
                .to_owned(),
        )
        .await?
        .into_iter()
        .map(|row| {
            let attributes: String = row.try_get("", "attributes")?;
            Ok(ExportedEvent {
                environment_id: row.try_get("", "environment_id")?,
                name: row.try_get("", "name")?,
                timestamp: row.try_get("", "timestamp")?,
                day: row.try_get("", "day")?,
                anonymous_id: row.try_get("", "anonymous_id").ok(),
                session_id: row.try_get("", "session_id").ok(),
                app_version: row.try_get("", "app_version").ok(),
                launcher_version: row.try_get("", "launcher_version").ok(),
                os: row.try_get("", "os").ok(),
                attributes: serde_json::from_str(&attributes).map_err(|error| {
                    DbErr::Custom(format!("invalid event attributes JSON: {error}"))
                })?,
                dedupe_key: row.try_get("", "dedupe_key").ok(),
                received_at: row.try_get("", "received_at")?,
            })
        })
        .collect::<Result<Vec<_>, DbErr>>()?;

    let logs = database
        .query_all(
            &Query::select()
                .columns(
                    [
                        "environment_id",
                        "level",
                        "message",
                        "logger",
                        "trace_id",
                        "span_id",
                        "timestamp",
                        "attributes",
                        "received_at",
                    ]
                    .map(Alias::new),
                )
                .from(Alias::new("logs"))
                .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
                .order_by(Alias::new("timestamp"), Order::Asc)
                .order_by(Alias::new("id"), Order::Asc)
                .to_owned(),
        )
        .await?
        .into_iter()
        .map(|row| {
            let attributes: String = row.try_get("", "attributes")?;
            Ok(ExportedLog {
                environment_id: row.try_get("", "environment_id")?,
                level: row.try_get("", "level")?,
                message: row.try_get("", "message")?,
                logger: row.try_get("", "logger").ok(),
                trace_id: row.try_get("", "trace_id").ok(),
                span_id: row.try_get("", "span_id").ok(),
                timestamp: row.try_get("", "timestamp")?,
                attributes: serde_json::from_str(&attributes).map_err(|error| {
                    DbErr::Custom(format!("invalid log attributes JSON: {error}"))
                })?,
                received_at: row.try_get("", "received_at")?,
            })
        })
        .collect::<Result<Vec<_>, DbErr>>()?;

    Ok(Some(ApplicationTransfer {
        exported_at: chrono::Utc::now().timestamp_millis(),
        application,
        environments,
        api_keys,
        events,
        logs,
    }))
}

pub(crate) async fn import_application(
    database: &impl ConnectionTrait,
    owner_user_id: Option<&str>,
    transfer: ApplicationTransfer,
) -> Result<ImportedApplication, DbErr> {
    let now = chrono::Utc::now().timestamp_millis();
    let application_id = Uuid::now_v7().to_string();

    let mut slug = transfer.application.slug.clone();
    let exists = Query::select()
        .column(Alias::new("id"))
        .from(Alias::new("applications"))
        .and_where(Expr::col(Alias::new("slug")).eq(&slug))
        .limit(1)
        .to_owned();
    if database.query_one(&exists).await?.is_some() {
        slug = format!("{}-imported-{}", slug, &application_id[..6]);
    }

    insert(
        database,
        "applications",
        &[
            "id",
            "name",
            "slug",
            "retention_days",
            "owner_user_id",
            "is_public",
            "description",
            "github_url",
            "website_url",
            "custom_header",
            "created_at",
        ],
        vec![
            application_id.clone().into(),
            transfer.application.name.into(),
            slug.into(),
            transfer.application.retention_days.into(),
            Value::from(owner_user_id.map(str::to_owned)),
            transfer.application.is_public.into(),
            Value::from(transfer.application.description),
            Value::from(transfer.application.github_url),
            Value::from(transfer.application.website_url),
            Value::from(transfer.application.custom_header),
            now.into(),
        ],
    )
    .await?;

    let mut environment_ids = HashMap::with_capacity(transfer.environments.len());
    for environment in transfer.environments {
        let new_id = Uuid::now_v7().to_string();
        insert(
            database,
            "environments",
            &["id", "application_id", "name", "slug", "created_at"],
            vec![
                new_id.clone().into(),
                application_id.clone().into(),
                environment.name.into(),
                environment.slug.into(),
                now.into(),
            ],
        )
        .await?;
        environment_ids.insert(environment.id, new_id);
    }

    let fallback_environment_id = if let Some(id) = environment_ids.values().next() {
        id.clone()
    } else {
        let id = Uuid::now_v7().to_string();
        insert(
            database,
            "environments",
            &["id", "application_id", "name", "slug", "created_at"],
            vec![
                id.clone().into(),
                application_id.clone().into(),
                "Production".into(),
                "production".into(),
                now.into(),
            ],
        )
        .await?;
        id
    };

    for key in transfer.api_keys {
        let environment_id = environment_ids
            .get(&key.environment_id)
            .unwrap_or(&fallback_environment_id);
        let scopes = serde_json::to_string(&key.scopes).map_err(|error| {
            DbErr::Custom(format!("API key scopes serialization failed: {error}"))
        })?;
        let mut key_hash = key.key_hash;
        let key_exists = Query::select()
            .column(Alias::new("id"))
            .from(Alias::new("api_keys"))
            .and_where(Expr::col(Alias::new("key_hash")).eq(&key_hash))
            .limit(1)
            .to_owned();
        if database.query_one(&key_exists).await?.is_some() {
            key_hash = hex::encode(Sha256::digest(
                format!("{key_hash}:{application_id}").as_bytes(),
            ));
        }
        insert(
            database,
            "api_keys",
            &[
                "id",
                "application_id",
                "environment_id",
                "name",
                "key_hash",
                "key_prefix",
                "scopes",
                "expires_at",
                "last_used_at",
                "revoked_at",
                "created_at",
            ],
            vec![
                Uuid::now_v7().to_string().into(),
                application_id.clone().into(),
                environment_id.to_owned().into(),
                key.name.into(),
                key_hash.into(),
                key.key_prefix.into(),
                scopes.into(),
                Value::from(key.expires_at),
                Value::from(key.last_used_at),
                Value::from(key.revoked_at),
                now.into(),
            ],
        )
        .await?;
    }

    for event in transfer.events {
        let environment_id = environment_ids
            .get(&event.environment_id)
            .unwrap_or(&fallback_environment_id);
        let attributes = serde_json::to_string(&event.attributes).map_err(|error| {
            DbErr::Custom(format!("event attributes serialization failed: {error}"))
        })?;
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
                application_id.clone().into(),
                environment_id.to_owned().into(),
                event.name.into(),
                event.timestamp.into(),
                event.day.into(),
                Value::from(event.anonymous_id),
                Value::from(event.session_id),
                Value::from(event.app_version),
                Value::from(event.launcher_version),
                Value::from(event.os),
                attributes.into(),
                Value::from(event.dedupe_key),
                event.received_at.into(),
            ],
        )
        .await?;
    }

    for log in transfer.logs {
        let environment_id = environment_ids
            .get(&log.environment_id)
            .unwrap_or(&fallback_environment_id);
        let attributes = serde_json::to_string(&log.attributes).map_err(|error| {
            DbErr::Custom(format!("log attributes serialization failed: {error}"))
        })?;
        insert(
            database,
            "logs",
            &[
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
            ],
            vec![
                Uuid::now_v7().to_string().into(),
                application_id.clone().into(),
                environment_id.to_owned().into(),
                log.level.into(),
                log.message.into(),
                Value::from(log.logger),
                Value::from(log.trace_id),
                Value::from(log.span_id),
                log.timestamp.into(),
                attributes.into(),
                log.received_at.into(),
            ],
        )
        .await?;
    }

    Ok(ImportedApplication {
        id: application_id,
        environment_ids,
        fallback_environment_id,
    })
}
