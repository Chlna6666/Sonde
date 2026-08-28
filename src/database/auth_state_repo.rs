use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr,
    sea_query::{Alias, Expr, ExprTrait, Query, Value},
};

use crate::auth;

use super::query::{insert, insert_batch_ignore_conflicts};

const SESSION_ABSOLUTE_MILLIS: i64 = 8 * 60 * 60 * 1_000;
const SESSION_IDLE_MILLIS: i64 = 30 * 60 * 1_000;
const SESSION_TOUCH_MILLIS: i64 = 60 * 1_000;
const TWO_FACTOR_TTL_MILLIS: i64 = 5 * 60 * 1_000;

#[derive(Clone, Debug)]
pub struct SharedSession {
    pub user_id: String,
    pub csrf_token: String,
}

pub async fn create_session(
    database: &DatabaseConnection,
    user_id: &str,
) -> Result<(String, String), DbErr> {
    let token = auth::random_token(32);
    let csrf_token = auth::random_token(24);
    let now = chrono::Utc::now().timestamp_millis();
    insert(
        database,
        "auth_sessions",
        &[
            "token_hash",
            "user_id",
            "csrf_token",
            "expires_at",
            "last_seen_at",
            "created_at",
        ],
        vec![
            Value::from(auth::token_hash(&token)),
            Value::from(user_id.to_owned()),
            Value::from(csrf_token.clone()),
            Value::from(now.saturating_add(SESSION_ABSOLUTE_MILLIS)),
            Value::from(now),
            Value::from(now),
        ],
    )
    .await?;
    Ok((token, csrf_token))
}

pub async fn session(
    database: &DatabaseConnection,
    token_hash: &str,
) -> Result<Option<SharedSession>, DbErr> {
    let now = chrono::Utc::now().timestamp_millis();
    let query = Query::select()
        .columns(
            ["user_id", "csrf_token", "expires_at", "last_seen_at"].map(Alias::new),
        )
        .from(Alias::new("auth_sessions"))
        .and_where(Expr::col(Alias::new("token_hash")).eq(token_hash))
        .limit(1)
        .to_owned();
    let Some(row) = database.query_one(&query).await? else {
        return Ok(None);
    };
    let expires_at: i64 = row.try_get("", "expires_at")?;
    let last_seen_at: i64 = row.try_get("", "last_seen_at")?;
    if expires_at <= now || last_seen_at.saturating_add(SESSION_IDLE_MILLIS) <= now {
        revoke_session(database, token_hash).await?;
        return Ok(None);
    }

    if now.saturating_sub(last_seen_at) >= SESSION_TOUCH_MILLIS {
        let touch = Query::update()
            .table(Alias::new("auth_sessions"))
            .value(Alias::new("last_seen_at"), now)
            .and_where(Expr::col(Alias::new("token_hash")).eq(token_hash))
            .and_where(
                Expr::col(Alias::new("last_seen_at"))
                    .lte(now.saturating_sub(SESSION_TOUCH_MILLIS)),
            )
            .to_owned();
        database.execute(&touch).await?;
    }

    Ok(Some(SharedSession {
        user_id: row.try_get("", "user_id")?,
        csrf_token: row.try_get("", "csrf_token")?,
    }))
}

pub async fn revoke_session(
    database: &DatabaseConnection,
    token_hash: &str,
) -> Result<(), DbErr> {
    let delete = Query::delete()
        .from_table(Alias::new("auth_sessions"))
        .and_where(Expr::col(Alias::new("token_hash")).eq(token_hash))
        .to_owned();
    database.execute(&delete).await?;
    Ok(())
}

pub async fn issue_2fa_temp_token(
    database: &DatabaseConnection,
    user_id: &str,
) -> Result<String, DbErr> {
    let token = format!("2fa_{}_{}", uuid::Uuid::now_v7(), auth::random_token(16));
    let now = chrono::Utc::now().timestamp_millis();
    insert(
        database,
        "auth_2fa_pending",
        &[
            "token_hash",
            "user_id",
            "expires_at",
            "consumed_at",
            "created_at",
        ],
        vec![
            Value::from(auth::token_hash(&token)),
            Value::from(user_id.to_owned()),
            Value::from(now.saturating_add(TWO_FACTOR_TTL_MILLIS)),
            Value::from(Option::<i64>::None),
            Value::from(now),
        ],
    )
    .await?;
    Ok(token)
}

pub async fn consume_2fa_temp_token(
    database: &DatabaseConnection,
    token: &str,
) -> Result<Option<String>, DbErr> {
    let token_hash = auth::token_hash(token);
    let now = chrono::Utc::now().timestamp_millis();
    let consume = Query::update()
        .table(Alias::new("auth_2fa_pending"))
        .value(Alias::new("consumed_at"), now)
        .and_where(Expr::col(Alias::new("token_hash")).eq(&token_hash))
        .and_where(Expr::col(Alias::new("consumed_at")).is_null())
        .and_where(Expr::col(Alias::new("expires_at")).gt(now))
        .to_owned();
    if database.execute(&consume).await?.rows_affected() != 1 {
        return Ok(None);
    }

    let query = Query::select()
        .column(Alias::new("user_id"))
        .from(Alias::new("auth_2fa_pending"))
        .and_where(Expr::col(Alias::new("token_hash")).eq(token_hash))
        .limit(1)
        .to_owned();
    Ok(database
        .query_one(&query)
        .await?
        .map(|row| row.try_get("", "user_id"))
        .transpose()?)
}

pub async fn consume_totp_step(
    database: &DatabaseConnection,
    user_id: &str,
    step: u64,
) -> Result<bool, DbErr> {
    let step = i64::try_from(step).map_err(|_| DbErr::Custom("TOTP step overflow".into()))?;
    let now = chrono::Utc::now().timestamp_millis();
    if advance_totp_step(database, user_id, step, now).await? {
        return Ok(true);
    }

    let inserted = insert_batch_ignore_conflicts(
        database,
        "auth_totp_replay",
        &["user_id", "last_step", "updated_at"],
        vec![vec![
            Value::from(user_id.to_owned()),
            Value::from(step),
            Value::from(now),
        ]],
        "user_id",
        "user_id",
    )
    .await?;
    if inserted > 0 {
        return Ok(true);
    }

    // Another instance may have inserted a lower step between our first UPDATE and INSERT.
    // Retry the monotonic conditional update once to preserve correctness under that race.
    advance_totp_step(database, user_id, step, now).await
}

async fn advance_totp_step(
    database: &DatabaseConnection,
    user_id: &str,
    step: i64,
    now: i64,
) -> Result<bool, DbErr> {
    let update = Query::update()
        .table(Alias::new("auth_totp_replay"))
        .value(Alias::new("last_step"), step)
        .value(Alias::new("updated_at"), now)
        .and_where(Expr::col(Alias::new("user_id")).eq(user_id))
        .and_where(Expr::col(Alias::new("last_step")).lt(step))
        .to_owned();
    Ok(database.execute(&update).await?.rows_affected() == 1)
}

pub async fn cleanup_expired(database: &DatabaseConnection) -> Result<(), DbErr> {
    let now = chrono::Utc::now().timestamp_millis();
    let idle_cutoff = now.saturating_sub(SESSION_IDLE_MILLIS);
    let sessions = Query::delete()
        .from_table(Alias::new("auth_sessions"))
        .and_where(
            Expr::col(Alias::new("expires_at"))
                .lt(now)
                .or(Expr::col(Alias::new("last_seen_at")).lt(idle_cutoff)),
        )
        .to_owned();
    database.execute(&sessions).await?;

    let two_factor = Query::delete()
        .from_table(Alias::new("auth_2fa_pending"))
        .and_where(
            Expr::col(Alias::new("expires_at"))
                .lt(now)
                .or(Expr::col(Alias::new("consumed_at")).is_not_null()),
        )
        .to_owned();
    database.execute(&two_factor).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::database;

    #[tokio::test]
    async fn shared_session_is_revocable() {
        let database = database::connect("sqlite::memory:").await.unwrap();
        database::migrate(&database).await.unwrap();
        let (token, csrf) = super::create_session(&database, "user-1").await.unwrap();
        let session = super::session(&database, &crate::auth::token_hash(&token))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(session.user_id, "user-1");
        assert_eq!(session.csrf_token, csrf);
        super::revoke_session(&database, &crate::auth::token_hash(&token))
            .await
            .unwrap();
        assert!(super::session(&database, &crate::auth::token_hash(&token))
            .await
            .unwrap()
            .is_none());
    }
}
