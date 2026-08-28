use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr, QueryResult, TransactionTrait,
    sea_query::{Alias, Expr, ExprTrait, Query, Value},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::permission::{
    ADMIN_PERMISSIONS, MANAGER_PERMISSIONS, OWNER_PERMISSIONS, PermissionGrant, USER_PERMISSIONS,
    VIEWER_PERMISSIONS,
};

use super::query::insert;

#[derive(Debug)]
pub struct UserCredential {
    pub id: String,
    pub email: String,
    pub username: String,
    pub password_hash: String,
    pub locale: String,
    pub active: bool,
    pub totp_secret: Option<String>,
    pub totp_enabled: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserSummary {
    pub id: String,
    pub email: String,
    pub username: String,
    pub locale: String,
    pub active: bool,
    pub created_at: i64,
    pub roles: Vec<String>,
    pub assigned_app_count: usize,
    pub totp_enabled: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleSummary {
    pub id: String,
    pub name: String,
    pub builtin: bool,
    pub permissions: Vec<String>,
}

pub async fn create_super_admin(
    database: &DatabaseConnection,
    email: &str,
    username: &str,
    password_hash: &str,
    locale: &str,
) -> Result<(), DbErr> {
    let transaction = database.begin().await?;
    let now = chrono::Utc::now().timestamp_millis();
    let norm_email = email.to_lowercase();

    let user_query = Query::select()
        .column(Alias::new("id"))
        .from(Alias::new("users"))
        .and_where(Expr::col(Alias::new("email")).eq(&norm_email))
        .limit(1)
        .to_owned();
    let user_id = if let Some(row) = transaction.query_one(&user_query).await? {
        let existing_id: String = row.try_get("", "id")?;
        let update_user = Query::update()
            .table(Alias::new("users"))
            .value(Alias::new("username"), username)
            .value(Alias::new("password_hash"), password_hash)
            .value(Alias::new("locale"), locale)
            .value(Alias::new("active"), true)
            .and_where(Expr::col(Alias::new("id")).eq(&existing_id))
            .to_owned();
        transaction.execute(&update_user).await?;
        existing_id
    } else {
        let id = Uuid::now_v7().to_string();
        insert(
            &transaction,
            "users",
            &[
                "id",
                "email",
                "username",
                "password_hash",
                "locale",
                "active",
                "created_at",
            ],
            vec![
                id.clone().into(),
                norm_email.into(),
                username.into(),
                password_hash.into(),
                locale.into(),
                true.into(),
                now.into(),
            ],
        )
        .await?;
        id
    };

    let mut super_admin_role_id = None;
    for (name, permissions) in [
        ("Super Admin", OWNER_PERMISSIONS),
        ("Admin", ADMIN_PERMISSIONS),
        ("User", USER_PERMISSIONS),
        ("Manager", MANAGER_PERMISSIONS),
        ("Viewer", VIEWER_PERMISSIONS),
    ] {
        let permissions_json = serde_json::to_string(permissions).map_err(json_error)?;
        let role_query = Query::select()
            .column(Alias::new("id"))
            .from(Alias::new("roles"))
            .and_where(Expr::col(Alias::new("name")).eq(name))
            .limit(1)
            .to_owned();
        let role_id = if let Some(row) = transaction.query_one(&role_query).await? {
            let existing_id: String = row.try_get("", "id")?;
            let update_role = Query::update()
                .table(Alias::new("roles"))
                .value(Alias::new("builtin"), true)
                .value(Alias::new("permissions"), permissions_json)
                .and_where(Expr::col(Alias::new("id")).eq(&existing_id))
                .to_owned();
            transaction.execute(&update_role).await?;
            existing_id
        } else {
            let id = Uuid::now_v7().to_string();
            insert(
                &transaction,
                "roles",
                &["id", "name", "builtin", "permissions", "created_at"],
                vec![
                    id.clone().into(),
                    name.into(),
                    true.into(),
                    permissions_json.into(),
                    now.into(),
                ],
            )
            .await?;
            id
        };
        if name == "Super Admin" {
            super_admin_role_id = Some(role_id);
        }
    }

    if let Some(role_id) = super_admin_role_id {
        let binding_query = Query::select()
            .column(Alias::new("id"))
            .from(Alias::new("role_bindings"))
            .and_where(Expr::col(Alias::new("user_id")).eq(&user_id))
            .and_where(Expr::col(Alias::new("role_id")).eq(&role_id))
            .and_where(Expr::col(Alias::new("application_id")).is_null())
            .limit(1)
            .to_owned();
        if transaction.query_one(&binding_query).await?.is_none() {
            insert(
                &transaction,
                "role_bindings",
                &["id", "user_id", "role_id", "application_id", "created_at"],
                vec![
                    Uuid::now_v7().to_string().into(),
                    user_id.into(),
                    role_id.into(),
                    Value::String(None),
                    now.into(),
                ],
            )
            .await?;
        }
    }

    let state_query = Query::select()
        .column(Alias::new("key"))
        .from(Alias::new("system_state"))
        .and_where(Expr::col(Alias::new("key")).eq("installed"))
        .limit(1)
        .to_owned();
    if transaction.query_one(&state_query).await?.is_some() {
        let update_state = Query::update()
            .table(Alias::new("system_state"))
            .value(Alias::new("value"), "true")
            .and_where(Expr::col(Alias::new("key")).eq("installed"))
            .to_owned();
        transaction.execute(&update_state).await?;
    } else {
        insert(
            &transaction,
            "system_state",
            &["key", "value"],
            vec!["installed".into(), "true".into()],
        )
        .await?;
    }

    transaction.commit().await
}

pub async fn user_by_identifier(
    database: &DatabaseConnection,
    identifier: &str,
) -> Result<Option<UserCredential>, DbErr> {
    let norm = identifier.trim().to_lowercase();
    let query = Query::select()
        .columns(
            [
                "id",
                "email",
                "username",
                "password_hash",
                "locale",
                "active",
                "totp_secret",
                "totp_enabled",
            ]
            .map(Alias::new),
        )
        .from(Alias::new("users"))
        .and_where(
            Expr::col(Alias::new("email"))
                .eq(&norm)
                .or(Expr::col(Alias::new("username")).eq(identifier.trim()))
                .or(Expr::col(Alias::new("username")).eq(&norm)),
        )
        .limit(1)
        .to_owned();
    database
        .query_one(&query)
        .await?
        .map(map_credential)
        .transpose()
}

pub async fn user_by_email(
    database: &DatabaseConnection,
    email: &str,
) -> Result<Option<UserCredential>, DbErr> {
    user_by_identifier(database, email).await
}

pub async fn user_by_id(
    database: &DatabaseConnection,
    user_id: &str,
) -> Result<Option<UserCredential>, DbErr> {
    let query = Query::select()
        .columns(
            [
                "id",
                "email",
                "username",
                "password_hash",
                "locale",
                "active",
                "totp_secret",
                "totp_enabled",
            ]
            .map(Alias::new),
        )
        .from(Alias::new("users"))
        .and_where(Expr::col(Alias::new("id")).eq(user_id))
        .limit(1)
        .to_owned();
    database
        .query_one(&query)
        .await?
        .map(map_credential)
        .transpose()
}

pub async fn role_names_for_user(
    database: &DatabaseConnection,
    user_id: &str,
) -> Result<Vec<String>, DbErr> {
    let query = Query::select()
        .column((Alias::new("roles"), Alias::new("name")))
        .from(Alias::new("role_bindings"))
        .inner_join(
            Alias::new("roles"),
            Expr::col((Alias::new("role_bindings"), Alias::new("role_id")))
                .equals((Alias::new("roles"), Alias::new("id"))),
        )
        .and_where(Expr::col((Alias::new("role_bindings"), Alias::new("user_id"))).eq(user_id))
        .to_owned();
    database
        .query_all(&query)
        .await?
        .into_iter()
        .map(|row| row.try_get("", "name"))
        .collect()
}

pub async fn update_password_hash(
    database: &DatabaseConnection,
    user_id: &str,
    password_hash: &str,
) -> Result<(), DbErr> {
    let query = Query::update()
        .table(Alias::new("users"))
        .value(Alias::new("password_hash"), password_hash)
        .and_where(Expr::col(Alias::new("id")).eq(user_id))
        .to_owned();
    database.execute(&query).await?;
    Ok(())
}

pub async fn grants_for_user(
    database: &DatabaseConnection,
    user_id: &str,
) -> Result<Vec<PermissionGrant>, DbErr> {
    let query = Query::select()
        .columns([
            (Alias::new("roles"), Alias::new("permissions")),
            (Alias::new("role_bindings"), Alias::new("application_id")),
        ])
        .from(Alias::new("role_bindings"))
        .inner_join(
            Alias::new("roles"),
            Expr::col((Alias::new("role_bindings"), Alias::new("role_id")))
                .equals((Alias::new("roles"), Alias::new("id"))),
        )
        .and_where(Expr::col((Alias::new("role_bindings"), Alias::new("user_id"))).eq(user_id))
        .to_owned();
    database
        .query_all(&query)
        .await?
        .into_iter()
        .map(map_grant)
        .collect()
}

pub async fn list_users(database: &DatabaseConnection) -> Result<Vec<UserSummary>, DbErr> {
    let users_query = Query::select()
        .columns(["id", "email", "username", "locale", "active", "created_at", "totp_enabled"].map(Alias::new))
        .from(Alias::new("users"))
        .order_by(Alias::new("created_at"), sea_orm::Order::Asc)
        .to_owned();
    let rows = database.query_all(&users_query).await?;
    let mut result = Vec::with_capacity(rows.len());

    for row in rows {
        let user_id: String = row.try_get("", "id")?;
        let roles = role_names_for_user(database, &user_id).await?;

        let app_count_query = Query::select()
            .expr(Expr::col(Alias::new("id")).count())
            .from(Alias::new("role_bindings"))
            .and_where(Expr::col(Alias::new("user_id")).eq(&user_id))
            .and_where(Expr::col(Alias::new("application_id")).is_not_null())
            .to_owned();
        let app_count: i64 = database
            .query_one(&app_count_query)
            .await?
            .and_then(|r| r.try_get("", "count").ok())
            .unwrap_or(0);

        let totp_enabled: bool = row
            .try_get::<bool>("", "totp_enabled")
            .unwrap_or_else(|_| row.try_get::<i32>("", "totp_enabled").map(|v| v == 1).unwrap_or(false));

        result.push(UserSummary {
            id: user_id,
            email: row.try_get("", "email")?,
            username: row.try_get("", "username")?,
            locale: row.try_get("", "locale")?,
            active: row.try_get("", "active")?,
            created_at: row.try_get("", "created_at")?,
            roles,
            assigned_app_count: app_count as usize,
            totp_enabled,
        });
    }
    Ok(result)
}

pub async fn create_user(
    database: &DatabaseConnection,
    email: &str,
    username: &str,
    password_hash: &str,
    locale: &str,
    role_name: &str,
) -> Result<String, DbErr> {
    let norm_email = email.trim().to_lowercase();
    let norm_username = username.trim();

    if user_by_identifier(database, &norm_email).await?.is_some() {
        return Err(DbErr::Custom("Email already in use".into()));
    }
    if user_by_identifier(database, norm_username).await?.is_some() {
        return Err(DbErr::Custom("Username already in use".into()));
    }

    let transaction = database.begin().await?;
    let now = chrono::Utc::now().timestamp_millis();
    let user_id = Uuid::now_v7().to_string();

    insert(
        &transaction,
        "users",
        &[
            "id",
            "email",
            "username",
            "password_hash",
            "locale",
            "active",
            "created_at",
        ],
        vec![
            user_id.clone().into(),
            norm_email.into(),
            norm_username.into(),
            password_hash.into(),
            locale.into(),
            true.into(),
            now.into(),
        ],
    )
    .await?;

    let role_query = Query::select()
        .column(Alias::new("id"))
        .from(Alias::new("roles"))
        .and_where(Expr::col(Alias::new("name")).eq(role_name))
        .limit(1)
        .to_owned();
    let role_id = if let Some(row) = transaction.query_one(&role_query).await? {
        row.try_get("", "id")?
    } else {
        let id = Uuid::now_v7().to_string();
        let perms = serde_json::to_string(USER_PERMISSIONS).map_err(json_error)?;
        insert(
            &transaction,
            "roles",
            &["id", "name", "builtin", "permissions", "created_at"],
            vec![
                id.clone().into(),
                role_name.into(),
                true.into(),
                perms.into(),
                now.into(),
            ],
        )
        .await?;
        id
    };

    insert(
        &transaction,
        "role_bindings",
        &["id", "user_id", "role_id", "application_id", "created_at"],
        vec![
            Uuid::now_v7().to_string().into(),
            user_id.clone().into(),
            role_id.into(),
            Value::String(None),
            now.into(),
        ],
    )
    .await?;

    transaction.commit().await?;
    Ok(user_id)
}

pub async fn update_user(
    database: &DatabaseConnection,
    user_id: &str,
    email: &str,
    username: &str,
    locale: &str,
    active: bool,
    role_name: Option<&str>,
) -> Result<(), DbErr> {
    let transaction = database.begin().await?;
    let norm_email = email.trim().to_lowercase();
    let norm_username = username.trim();

    let update = Query::update()
        .table(Alias::new("users"))
        .value(Alias::new("email"), norm_email)
        .value(Alias::new("username"), norm_username)
        .value(Alias::new("locale"), locale)
        .value(Alias::new("active"), active)
        .and_where(Expr::col(Alias::new("id")).eq(user_id))
        .to_owned();
    transaction.execute(&update).await?;

    if let Some(role) = role_name {
        let role_query = Query::select()
            .column(Alias::new("id"))
            .from(Alias::new("roles"))
            .and_where(Expr::col(Alias::new("name")).eq(role))
            .limit(1)
            .to_owned();
        if let Some(row) = transaction.query_one(&role_query).await? {
            let role_id: String = row.try_get("", "id")?;
            let delete_binding = Query::delete()
                .from_table(Alias::new("role_bindings"))
                .and_where(Expr::col(Alias::new("user_id")).eq(user_id))
                .and_where(Expr::col(Alias::new("application_id")).is_null())
                .to_owned();
            transaction.execute(&delete_binding).await?;

            let now = chrono::Utc::now().timestamp_millis();
            insert(
                &transaction,
                "role_bindings",
                &["id", "user_id", "role_id", "application_id", "created_at"],
                vec![
                    Uuid::now_v7().to_string().into(),
                    user_id.into(),
                    role_id.into(),
                    Value::String(None),
                    now.into(),
                ],
            )
            .await?;
        }
    }

    transaction.commit().await
}

pub async fn delete_user(database: &DatabaseConnection, user_id: &str) -> Result<(), DbErr> {
    let transaction = database.begin().await?;
    let delete_bindings = Query::delete()
        .from_table(Alias::new("role_bindings"))
        .and_where(Expr::col(Alias::new("user_id")).eq(user_id))
        .to_owned();
    transaction.execute(&delete_bindings).await?;

    let delete_user = Query::delete()
        .from_table(Alias::new("users"))
        .and_where(Expr::col(Alias::new("id")).eq(user_id))
        .to_owned();
    transaction.execute(&delete_user).await?;

    transaction.commit().await
}

pub async fn list_roles(database: &DatabaseConnection) -> Result<Vec<RoleSummary>, DbErr> {
    let query = Query::select()
        .columns(["id", "name", "builtin", "permissions"].map(Alias::new))
        .from(Alias::new("roles"))
        .order_by(Alias::new("name"), sea_orm::Order::Asc)
        .to_owned();
    let rows = database.query_all(&query).await?;
    let mut roles = Vec::with_capacity(rows.len());
    for row in rows {
        let perms_str: String = row.try_get("", "permissions")?;
        let permissions: Vec<String> = serde_json::from_str(&perms_str).unwrap_or_default();
        roles.push(RoleSummary {
            id: row.try_get("", "id")?,
            name: row.try_get("", "name")?,
            builtin: row.try_get("", "builtin")?,
            permissions,
        });
    }
    Ok(roles)
}

pub async fn enable_totp(
    database: &DatabaseConnection,
    user_id: &str,
    secret: &str,
) -> Result<(), DbErr> {
    let update = Query::update()
        .table(Alias::new("users"))
        .value(Alias::new("totp_secret"), secret)
        .value(Alias::new("totp_enabled"), true)
        .and_where(Expr::col(Alias::new("id")).eq(user_id))
        .to_owned();
    database.execute(&update).await?;
    Ok(())
}

pub async fn disable_totp(database: &DatabaseConnection, user_id: &str) -> Result<(), DbErr> {
    let update = Query::update()
        .table(Alias::new("users"))
        .value(Alias::new("totp_secret"), Option::<String>::None)
        .value(Alias::new("totp_enabled"), false)
        .and_where(Expr::col(Alias::new("id")).eq(user_id))
        .to_owned();
    database.execute(&update).await?;
    Ok(())
}

pub async fn get_totp_info(
    database: &DatabaseConnection,
    user_id: &str,
) -> Result<(bool, Option<String>), DbErr> {
    let query = Query::select()
        .columns(["totp_enabled", "totp_secret"].map(Alias::new))
        .from(Alias::new("users"))
        .and_where(Expr::col(Alias::new("id")).eq(user_id))
        .limit(1)
        .to_owned();
    if let Some(row) = database.query_one(&query).await? {
        let enabled = row
            .try_get::<bool>("", "totp_enabled")
            .unwrap_or_else(|_| row.try_get::<i32>("", "totp_enabled").map(|v| v == 1).unwrap_or(false));
        let secret: Option<String> = row.try_get("", "totp_secret").ok();
        Ok((enabled, secret))
    } else {
        Ok((false, None))
    }
}

fn map_credential(row: QueryResult) -> Result<UserCredential, DbErr> {
    let totp_enabled = row
        .try_get::<bool>("", "totp_enabled")
        .unwrap_or_else(|_| row.try_get::<i32>("", "totp_enabled").map(|v| v == 1).unwrap_or(false));
    Ok(UserCredential {
        id: row.try_get("", "id")?,
        email: row.try_get("", "email")?,
        username: row.try_get("", "username")?,
        password_hash: row.try_get("", "password_hash")?,
        locale: row.try_get("", "locale")?,
        active: row.try_get("", "active")?,
        totp_secret: row.try_get("", "totp_secret").ok(),
        totp_enabled,
    })
}

fn map_grant(row: QueryResult) -> Result<PermissionGrant, DbErr> {
    let permissions: String = row.try_get("", "permissions")?;
    Ok(PermissionGrant {
        permissions: serde_json::from_str(&permissions).map_err(json_error)?,
        application_id: row.try_get("", "application_id")?,
    })
}

fn json_error(error: serde_json::Error) -> DbErr {
    DbErr::Custom(error.to_string())
}
