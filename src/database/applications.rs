use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr, QueryResult, TransactionTrait,
    sea_query::{Alias, Expr, ExprTrait, Query, Value},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::query::insert;

#[derive(Clone, Debug, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationSummary {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub retention_days: i32,
    pub owner_user_id: Option<String>,
    pub is_public: bool,
    pub description: Option<String>,
    pub github_url: Option<String>,
    pub website_url: Option<String>,
    pub custom_header: Option<String>,
    pub created_at: i64,
}

#[derive(Clone, Debug, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicApplicationInfo {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub is_public: bool,
    pub description: Option<String>,
    pub github_url: Option<String>,
    pub website_url: Option<String>,
    pub custom_header: Option<String>,
    pub created_at: i64,
}

#[derive(Clone, Debug, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppMemberSummary {
    pub user_id: String,
    pub username: String,
    pub email: String,
    pub role: String,
    pub granted_at: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentSummary {
    pub id: String,
    pub application_id: String,
    pub name: String,
    pub slug: String,
}

#[derive(Debug)]
pub struct ApiKeyContext {
    pub id: String,
    pub application_id: String,
    pub environment_id: String,
    pub scopes: Vec<String>,
}

pub async fn create_application(
    database: &DatabaseConnection,
    name: &str,
    slug: &str,
    owner_user_id: Option<&str>,
) -> Result<(String, String), DbErr> {
    let transaction = database.begin().await?;
    let application_id = Uuid::now_v7().to_string();
    let environment_id = Uuid::now_v7().to_string();
    let now = chrono::Utc::now().timestamp_millis();
    insert(
        &transaction,
        "applications",
        &[
            "id",
            "name",
            "slug",
            "retention_days",
            "owner_user_id",
            "is_public",
            "created_at",
        ],
        vec![
            application_id.clone().into(),
            name.into(),
            slug.into(),
            365.into(),
            owner_user_id.map(Into::into).unwrap_or(Value::String(None)),
            false.into(),
            now.into(),
        ],
    )
    .await?;
    insert(
        &transaction,
        "environments",
        &["id", "application_id", "name", "slug", "created_at"],
        vec![
            environment_id.clone().into(),
            application_id.clone().into(),
            "Production".into(),
            "production".into(),
            now.into(),
        ],
    )
    .await?;
    transaction.commit().await?;
    Ok((application_id, environment_id))
}

pub async fn list_applications(
    database: &DatabaseConnection,
    user_id: Option<&str>,
    is_admin: bool,
) -> Result<Vec<ApplicationSummary>, DbErr> {
    let mut query = Query::select()
        .columns(
            [
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
            ]
            .map(Alias::new),
        )
        .from(Alias::new("applications"))
        .to_owned();

    if !is_admin && let Some(uid) = user_id {
        let granted_apps = Query::select()
            .column(Alias::new("application_id"))
            .from(Alias::new("role_bindings"))
            .and_where(Expr::col(Alias::new("user_id")).eq(uid))
            .and_where(Expr::col(Alias::new("application_id")).is_not_null())
            .to_owned();
        query.and_where(
            Expr::col(Alias::new("owner_user_id"))
                .eq(uid)
                .or(Expr::col(Alias::new("id")).in_subquery(granted_apps)),
        );
    }

    query.order_by(Alias::new("created_at"), sea_orm::sea_query::Order::Desc);
    database
        .query_all(&query)
        .await?
        .into_iter()
        .map(map_application)
        .collect()
}

pub async fn get_public_application_by_slug(
    database: &DatabaseConnection,
    slug: &str,
) -> Result<Option<PublicApplicationInfo>, DbErr> {
    let query = Query::select()
        .columns(
            [
                "id",
                "name",
                "slug",
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
        .and_where(Expr::col(Alias::new("slug")).eq(slug))
        .limit(1)
        .to_owned();
    let Some(row) = database.query_one(&query).await? else {
        return Ok(None);
    };
    let is_public: bool = row.try_get("", "is_public").unwrap_or(false);
    if !is_public {
        return Ok(None);
    }
    Ok(Some(PublicApplicationInfo {
        id: row.try_get("", "id")?,
        name: row.try_get("", "name")?,
        slug: row.try_get("", "slug")?,
        is_public,
        description: row.try_get("", "description")?,
        github_url: row.try_get("", "github_url")?,
        website_url: row.try_get("", "website_url")?,
        custom_header: row.try_get("", "custom_header")?,
        created_at: row.try_get("", "created_at")?,
    }))
}

pub async fn list_environments(
    database: &DatabaseConnection,
    application_id: &str,
) -> Result<Vec<EnvironmentSummary>, DbErr> {
    let query = Query::select()
        .columns(["id", "application_id", "name", "slug"].map(Alias::new))
        .from(Alias::new("environments"))
        .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
        .order_by(Alias::new("created_at"), sea_orm::sea_query::Order::Asc)
        .to_owned();
    database
        .query_all(&query)
        .await?
        .into_iter()
        .map(|row| {
            Ok(EnvironmentSummary {
                id: row.try_get("", "id")?,
                application_id: row.try_get("", "application_id")?,
                name: row.try_get("", "name")?,
                slug: row.try_get("", "slug")?,
            })
        })
        .collect()
}

pub async fn create_api_key(
    database: &DatabaseConnection,
    application_id: &str,
    environment_id: &str,
    name: &str,
    key_hash: &str,
    key_prefix: &str,
    scopes: &[String],
) -> Result<String, DbErr> {
    let id = Uuid::now_v7().to_string();
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
            id.clone().into(),
            application_id.into(),
            environment_id.into(),
            name.into(),
            key_hash.into(),
            key_prefix.into(),
            serde_json::to_string(scopes).map_err(json_error)?.into(),
            Value::BigInt(None),
            Value::BigInt(None),
            Value::BigInt(None),
            chrono::Utc::now().timestamp_millis().into(),
        ],
    )
    .await?;
    Ok(id)
}

pub async fn api_key_context(
    database: &DatabaseConnection,
    key_hash: &str,
    now: i64,
) -> Result<Option<ApiKeyContext>, DbErr> {
    let query = Query::select()
        .columns(["id", "application_id", "environment_id", "scopes"].map(Alias::new))
        .from(Alias::new("api_keys"))
        .and_where(Expr::col(Alias::new("key_hash")).eq(key_hash))
        .and_where(Expr::col(Alias::new("revoked_at")).is_null())
        .and_where(
            Expr::col(Alias::new("expires_at"))
                .is_null()
                .or(Expr::col(Alias::new("expires_at")).gt(now)),
        )
        .limit(1)
        .to_owned();
    let Some(row) = database.query_one(&query).await? else {
        return Ok(None);
    };
    let id: String = row.try_get("", "id")?;
    let update = Query::update()
        .table(Alias::new("api_keys"))
        .value(Alias::new("last_used_at"), now)
        .and_where(Expr::col(Alias::new("id")).eq(id.clone()))
        .to_owned();
    database.execute(&update).await?;
    Ok(Some(ApiKeyContext {
        id,
        application_id: row.try_get("", "application_id")?,
        environment_id: row.try_get("", "environment_id")?,
        scopes: serde_json::from_str(&row.try_get::<String>("", "scopes")?).map_err(json_error)?,
    }))
}

pub async fn audit(
    database: &DatabaseConnection,
    actor: Option<&str>,
    action: &str,
    resource_type: &str,
    resource_id: Option<&str>,
) -> Result<(), DbErr> {
    insert(
        database,
        "audit_log",
        &[
            "id",
            "actor_user_id",
            "action",
            "resource_type",
            "resource_id",
            "metadata",
            "created_at",
        ],
        vec![
            Uuid::now_v7().to_string().into(),
            actor.map(str::to_owned).into(),
            action.into(),
            resource_type.into(),
            resource_id.map(str::to_owned).into(),
            "{}".into(),
            chrono::Utc::now().timestamp_millis().into(),
        ],
    )
    .await?;
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditLogRecord {
    pub id: String,
    pub actor_user_id: Option<String>,
    pub actor_username: Option<String>,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<String>,
    pub metadata: serde_json::Value,
    pub created_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditLogPage {
    pub items: Vec<AuditLogRecord>,
    pub page: u64,
    pub page_size: u64,
    pub has_more: bool,
}

pub async fn list_audit_logs(
    database: &DatabaseConnection,
    page: u64,
    page_size: u64,
    action: Option<&str>,
    resource_type: Option<&str>,
) -> Result<AuditLogPage, DbErr> {
    let mut query = Query::select();
    query
        .columns([
            (Alias::new("al"), Alias::new("id")),
            (Alias::new("al"), Alias::new("actor_user_id")),
            (Alias::new("al"), Alias::new("action")),
            (Alias::new("al"), Alias::new("resource_type")),
            (Alias::new("al"), Alias::new("resource_id")),
            (Alias::new("al"), Alias::new("metadata")),
            (Alias::new("al"), Alias::new("created_at")),
        ])
        .expr_as(
            Expr::col((Alias::new("u"), Alias::new("username"))),
            Alias::new("actor_username"),
        )
        .from_as(Alias::new("audit_log"), Alias::new("al"))
        .left_join(
            Alias::new("users"),
            Expr::col((Alias::new("al"), Alias::new("actor_user_id")))
                .equals((Alias::new("u"), Alias::new("id"))),
        )
        .order_by(
            (Alias::new("al"), Alias::new("created_at")),
            sea_orm::Order::Desc,
        )
        .limit(page_size + 1)
        .offset(page.saturating_sub(1) * page_size);

    if let Some(act) = action
        && !act.trim().is_empty()
    {
        query.and_where(Expr::col((Alias::new("al"), Alias::new("action"))).eq(act));
    }
    if let Some(rt) = resource_type
        && !rt.trim().is_empty()
    {
        query.and_where(Expr::col((Alias::new("al"), Alias::new("resource_type"))).eq(rt));
    }

    let rows = database.query_all(&query).await?;
    let has_more = rows.len() as u64 > page_size;
    let mut items = Vec::with_capacity(rows.len());

    for row in rows.into_iter().take(page_size as usize) {
        let meta_str: String = row.try_get("", "metadata").unwrap_or_else(|_| "{}".into());
        let metadata: serde_json::Value =
            serde_json::from_str(&meta_str).unwrap_or(serde_json::Value::Null);
        items.push(AuditLogRecord {
            id: row.try_get("", "id")?,
            actor_user_id: row.try_get("", "actor_user_id").ok(),
            actor_username: row.try_get("", "actor_username").ok(),
            action: row.try_get("", "action")?,
            resource_type: row.try_get("", "resource_type")?,
            resource_id: row.try_get("", "resource_id").ok(),
            metadata,
            created_at: row.try_get("", "created_at")?,
        });
    }

    Ok(AuditLogPage {
        items,
        page,
        page_size,
        has_more,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyDetail {
    pub id: String,
    pub application_id: String,
    pub environment_id: String,
    pub environment_name: String,
    pub name: String,
    pub key_prefix: String,
    pub scopes: Vec<String>,
    pub expires_at: Option<i64>,
    pub last_used_at: Option<i64>,
    pub revoked_at: Option<i64>,
    pub created_at: i64,
    pub is_active: bool,
}

pub async fn get_application(
    database: &DatabaseConnection,
    application_id: &str,
) -> Result<Option<ApplicationSummary>, DbErr> {
    let query = Query::select()
        .columns(
            [
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
            ]
            .map(Alias::new),
        )
        .from(Alias::new("applications"))
        .and_where(Expr::col(Alias::new("id")).eq(application_id))
        .limit(1)
        .to_owned();
    database
        .query_one(&query)
        .await?
        .map(map_application)
        .transpose()
}

#[derive(Debug, Default)]
pub struct UpdateApplicationParams<'a> {
    pub name: &'a str,
    pub slug: &'a str,
    pub retention_days: i32,
    pub is_public: Option<bool>,
    pub description: Option<Option<String>>,
    pub github_url: Option<Option<String>>,
    pub website_url: Option<Option<String>>,
    pub custom_header: Option<Option<String>>,
}

pub async fn update_application(
    database: &DatabaseConnection,
    application_id: &str,
    params: UpdateApplicationParams<'_>,
) -> Result<(), DbErr> {
    let mut update = Query::update();
    update
        .table(Alias::new("applications"))
        .value(Alias::new("name"), params.name)
        .value(Alias::new("slug"), params.slug)
        .value(Alias::new("retention_days"), params.retention_days);

    if let Some(pub_val) = params.is_public {
        update.value(Alias::new("is_public"), pub_val);
    }
    if let Some(desc) = params.description {
        update.value(
            Alias::new("description"),
            desc.map(Into::into).unwrap_or(Value::String(None)),
        );
    }
    if let Some(gh) = params.github_url {
        update.value(
            Alias::new("github_url"),
            gh.map(Into::into).unwrap_or(Value::String(None)),
        );
    }
    if let Some(web) = params.website_url {
        update.value(
            Alias::new("website_url"),
            web.map(Into::into).unwrap_or(Value::String(None)),
        );
    }
    if let Some(hdr) = params.custom_header {
        update.value(
            Alias::new("custom_header"),
            hdr.map(Into::into).unwrap_or(Value::String(None)),
        );
    }

    update.and_where(Expr::col(Alias::new("id")).eq(application_id));
    database.execute(&update.to_owned()).await?;
    Ok(())
}

pub async fn delete_application(
    database: &DatabaseConnection,
    application_id: &str,
) -> Result<(), DbErr> {
    let transaction = database.begin().await?;
    for table in [
        "events",
        "metric_points",
        "logs",
        "daily_aggregates",
        "alert_rules",
        "import_runs",
        "api_keys",
        "environments",
        "role_bindings",
    ] {
        let delete = Query::delete()
            .from_table(Alias::new(table))
            .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
            .to_owned();
        transaction.execute(&delete).await?;
    }
    let delete_app = Query::delete()
        .from_table(Alias::new("applications"))
        .and_where(Expr::col(Alias::new("id")).eq(application_id))
        .to_owned();
    transaction.execute(&delete_app).await?;
    transaction.commit().await?;
    Ok(())
}

pub async fn list_application_members(
    database: &DatabaseConnection,
    application_id: &str,
) -> Result<Vec<AppMemberSummary>, DbErr> {
    let query = Query::select()
        .columns([
            (Alias::new("users"), Alias::new("id")),
            (Alias::new("users"), Alias::new("username")),
            (Alias::new("users"), Alias::new("email")),
            (Alias::new("roles"), Alias::new("name")),
            (Alias::new("role_bindings"), Alias::new("created_at")),
        ])
        .from(Alias::new("role_bindings"))
        .inner_join(
            Alias::new("users"),
            Expr::col((Alias::new("role_bindings"), Alias::new("user_id")))
                .equals((Alias::new("users"), Alias::new("id"))),
        )
        .inner_join(
            Alias::new("roles"),
            Expr::col((Alias::new("role_bindings"), Alias::new("role_id")))
                .equals((Alias::new("roles"), Alias::new("id"))),
        )
        .and_where(
            Expr::col((Alias::new("role_bindings"), Alias::new("application_id")))
                .eq(application_id),
        )
        .to_owned();

    database
        .query_all(&query)
        .await?
        .into_iter()
        .map(|row| {
            Ok(AppMemberSummary {
                user_id: row.try_get("", "id")?,
                username: row.try_get("", "username")?,
                email: row.try_get("", "email")?,
                role: row.try_get("", "name")?,
                granted_at: row.try_get("", "created_at")?,
            })
        })
        .collect()
}

pub async fn grant_application_access(
    database: &DatabaseConnection,
    application_id: &str,
    user_id: &str,
    role_name: &str,
) -> Result<(), DbErr> {
    let transaction = database.begin().await?;
    let role_query = Query::select()
        .column(Alias::new("id"))
        .from(Alias::new("roles"))
        .and_where(Expr::col(Alias::new("name")).eq(role_name))
        .limit(1)
        .to_owned();
    let Some(role_row) = transaction.query_one(&role_query).await? else {
        return Err(DbErr::Custom(format!("Role '{role_name}' not found")));
    };
    let role_id: String = role_row.try_get("", "id")?;

    let delete_existing = Query::delete()
        .from_table(Alias::new("role_bindings"))
        .and_where(Expr::col(Alias::new("user_id")).eq(user_id))
        .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
        .to_owned();
    transaction.execute(&delete_existing).await?;

    let now = chrono::Utc::now().timestamp_millis();
    insert(
        &transaction,
        "role_bindings",
        &["id", "user_id", "role_id", "application_id", "created_at"],
        vec![
            Uuid::now_v7().to_string().into(),
            user_id.into(),
            role_id.into(),
            application_id.into(),
            now.into(),
        ],
    )
    .await?;

    transaction.commit().await
}

pub async fn revoke_application_access(
    database: &DatabaseConnection,
    application_id: &str,
    user_id: &str,
) -> Result<bool, DbErr> {
    let query = Query::delete()
        .from_table(Alias::new("role_bindings"))
        .and_where(Expr::col(Alias::new("user_id")).eq(user_id))
        .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
        .to_owned();
    let res = database.execute(&query).await?;
    Ok(res.rows_affected() > 0)
}

pub async fn get_user_assigned_applications(
    database: &DatabaseConnection,
    user_id: &str,
) -> Result<Vec<String>, DbErr> {
    let query = Query::select()
        .column(Alias::new("application_id"))
        .from(Alias::new("role_bindings"))
        .and_where(Expr::col(Alias::new("user_id")).eq(user_id))
        .and_where(Expr::col(Alias::new("application_id")).is_not_null())
        .to_owned();
    database
        .query_all(&query)
        .await?
        .into_iter()
        .map(|row| row.try_get("", "application_id"))
        .collect()
}

pub async fn set_user_assigned_applications(
    database: &DatabaseConnection,
    user_id: &str,
    application_ids: &[String],
    default_role: &str,
) -> Result<(), DbErr> {
    let transaction = database.begin().await?;
    let role_query = Query::select()
        .column(Alias::new("id"))
        .from(Alias::new("roles"))
        .and_where(Expr::col(Alias::new("name")).eq(default_role))
        .limit(1)
        .to_owned();
    let Some(role_row) = transaction.query_one(&role_query).await? else {
        return Err(DbErr::Custom(format!("Role '{default_role}' not found")));
    };
    let role_id: String = role_row.try_get("", "id")?;

    let delete_existing = Query::delete()
        .from_table(Alias::new("role_bindings"))
        .and_where(Expr::col(Alias::new("user_id")).eq(user_id))
        .and_where(Expr::col(Alias::new("application_id")).is_not_null())
        .to_owned();
    transaction.execute(&delete_existing).await?;

    let now = chrono::Utc::now().timestamp_millis();
    for app_id in application_ids {
        insert(
            &transaction,
            "role_bindings",
            &["id", "user_id", "role_id", "application_id", "created_at"],
            vec![
                Uuid::now_v7().to_string().into(),
                user_id.into(),
                role_id.clone().into(),
                app_id.clone().into(),
                now.into(),
            ],
        )
        .await?;
    }

    transaction.commit().await
}

pub async fn list_api_keys(
    database: &DatabaseConnection,
    application_id: &str,
) -> Result<Vec<ApiKeyDetail>, DbErr> {
    let query = Query::select()
        .columns(
            [
                "id",
                "application_id",
                "environment_id",
                "name",
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
        .order_by(Alias::new("created_at"), sea_orm::sea_query::Order::Desc)
        .to_owned();
    let rows = database.query_all(&query).await?;
    let envs = list_environments(database, application_id).await?;
    let env_map: std::collections::HashMap<String, String> =
        envs.into_iter().map(|env| (env.id, env.name)).collect();

    let now = chrono::Utc::now().timestamp_millis();
    let mut details = Vec::with_capacity(rows.len());
    for row in rows {
        let env_id: String = row.try_get("", "environment_id")?;
        let env_name = env_map
            .get(&env_id)
            .cloned()
            .unwrap_or_else(|| "Unknown".into());
        let expires_at: Option<i64> = row.try_get("", "expires_at")?;
        let revoked_at: Option<i64> = row.try_get("", "revoked_at")?;
        let is_active = revoked_at.is_none() && expires_at.is_none_or(|exp| exp > now);
        let scopes_str: String = row.try_get("", "scopes")?;
        let scopes: Vec<String> = serde_json::from_str(&scopes_str).unwrap_or_default();
        details.push(ApiKeyDetail {
            id: row.try_get("", "id")?,
            application_id: row.try_get("", "application_id")?,
            environment_id: env_id,
            environment_name: env_name,
            name: row.try_get("", "name")?,
            key_prefix: row.try_get("", "key_prefix")?,
            scopes,
            expires_at,
            last_used_at: row.try_get("", "last_used_at")?,
            revoked_at,
            created_at: row.try_get("", "created_at")?,
            is_active,
        });
    }
    Ok(details)
}

pub async fn get_api_key(
    database: &DatabaseConnection,
    application_id: &str,
    key_id: &str,
) -> Result<Option<ApiKeyDetail>, DbErr> {
    let query = Query::select()
        .columns(
            [
                "id",
                "application_id",
                "environment_id",
                "name",
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
        .and_where(Expr::col(Alias::new("id")).eq(key_id))
        .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
        .limit(1)
        .to_owned();
    let Some(row) = database.query_one(&query).await? else {
        return Ok(None);
    };
    let env_id: String = row.try_get("", "environment_id")?;
    let env_name = list_environments(database, application_id)
        .await?
        .into_iter()
        .find(|e| e.id == env_id)
        .map(|e| e.name)
        .unwrap_or_else(|| "Unknown".into());
    let expires_at: Option<i64> = row.try_get("", "expires_at")?;
    let revoked_at: Option<i64> = row.try_get("", "revoked_at")?;
    let now = chrono::Utc::now().timestamp_millis();
    let is_active = revoked_at.is_none() && expires_at.is_none_or(|exp| exp > now);
    let scopes_str: String = row.try_get("", "scopes")?;
    let scopes: Vec<String> = serde_json::from_str(&scopes_str).unwrap_or_default();
    Ok(Some(ApiKeyDetail {
        id: row.try_get("", "id")?,
        application_id: row.try_get("", "application_id")?,
        environment_id: env_id,
        environment_name: env_name,
        name: row.try_get("", "name")?,
        key_prefix: row.try_get("", "key_prefix")?,
        scopes,
        expires_at,
        last_used_at: row.try_get("", "last_used_at")?,
        revoked_at,
        created_at: row.try_get("", "created_at")?,
        is_active,
    }))
}

pub async fn revoke_api_key(
    database: &DatabaseConnection,
    application_id: &str,
    key_id: &str,
) -> Result<bool, DbErr> {
    let now = chrono::Utc::now().timestamp_millis();
    let update = Query::update()
        .table(Alias::new("api_keys"))
        .value(Alias::new("revoked_at"), now)
        .and_where(Expr::col(Alias::new("id")).eq(key_id))
        .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
        .and_where(Expr::col(Alias::new("revoked_at")).is_null())
        .to_owned();
    let res = database.execute(&update).await?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_api_key(
    database: &DatabaseConnection,
    application_id: &str,
    key_id: &str,
) -> Result<bool, DbErr> {
    let delete = Query::delete()
        .from_table(Alias::new("api_keys"))
        .and_where(Expr::col(Alias::new("id")).eq(key_id))
        .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
        .to_owned();
    let res = database.execute(&delete).await?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_revoked_api_keys(
    database: &DatabaseConnection,
    application_id: &str,
) -> Result<u64, DbErr> {
    let now = chrono::Utc::now().timestamp_millis();
    let delete = Query::delete()
        .from_table(Alias::new("api_keys"))
        .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
        .and_where(
            Expr::col(Alias::new("revoked_at"))
                .is_not_null()
                .or(Expr::col(Alias::new("expires_at")).lte(now)),
        )
        .to_owned();
    let res = database.execute(&delete).await?;
    Ok(res.rows_affected())
}

fn map_application(row: QueryResult) -> Result<ApplicationSummary, DbErr> {
    Ok(ApplicationSummary {
        id: row.try_get("", "id")?,
        name: row.try_get("", "name")?,
        slug: row.try_get("", "slug")?,
        retention_days: row.try_get("", "retention_days")?,
        owner_user_id: row.try_get("", "owner_user_id").ok(),
        is_public: row.try_get("", "is_public").unwrap_or(false),
        description: row.try_get("", "description").ok(),
        github_url: row.try_get("", "github_url").ok(),
        website_url: row.try_get("", "website_url").ok(),
        custom_header: row.try_get("", "custom_header").ok(),
        created_at: row.try_get("", "created_at")?,
    })
}

fn json_error(error: serde_json::Error) -> DbErr {
    DbErr::Custom(error.to_string())
}
