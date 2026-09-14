use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr,
    sea_query::{Alias, Expr, ExprTrait, Query},
};

use super::applications::ApiKeyContext;

const LAST_USED_FLUSH_INTERVAL_MILLIS: i64 = 60_000;

/// Resolve an ingest API key without turning every telemetry request into an extra write.
///
/// `last_used_at` is operational metadata, not part of authorization correctness. Updating it once
/// per minute is sufficient for the UI while avoiding a writer lock/transaction on every batch,
/// which is particularly important for SQLite deployments.
pub async fn api_key_context(
    database: &DatabaseConnection,
    key_hash: &str,
    now: i64,
) -> Result<Option<ApiKeyContext>, DbErr> {
    let query = Query::select()
        .columns(
            [
                "id",
                "application_id",
                "environment_id",
                "scopes",
                "last_used_at",
            ]
            .map(Alias::new),
        )
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
    let last_used_at: Option<i64> = row.try_get("", "last_used_at")?;
    if last_used_at.is_none_or(|last| now.saturating_sub(last) >= LAST_USED_FLUSH_INTERVAL_MILLIS) {
        let update = Query::update()
            .table(Alias::new("api_keys"))
            .value(Alias::new("last_used_at"), now)
            .and_where(Expr::col(Alias::new("id")).eq(id.clone()))
            .and_where(
                Expr::col(Alias::new("last_used_at"))
                    .is_null()
                    .or(Expr::col(Alias::new("last_used_at"))
                        .lte(now.saturating_sub(LAST_USED_FLUSH_INTERVAL_MILLIS))),
            )
            .to_owned();
        database.execute(&update).await?;
    }

    let scopes: String = row.try_get("", "scopes")?;
    Ok(Some(ApiKeyContext {
        id,
        application_id: row.try_get("", "application_id")?,
        environment_id: row.try_get("", "environment_id")?,
        scopes: serde_json::from_str(&scopes).map_err(|error| DbErr::Custom(error.to_string()))?,
    }))
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    use crate::database::{self, applications};

    #[tokio::test]
    async fn ingest_key_last_used_is_throttled() -> Result<(), Box<dyn std::error::Error>> {
        let database = database::connect("sqlite::memory:").await?;
        database::migrate(&database).await?;
        let (app_id, env_id) =
            applications::create_application(&database, "Demo", "demo", None).await?;
        let raw_key = "sonde_ingest_test_key";
        let key_hash = hex::encode(Sha256::digest(raw_key.as_bytes()));
        applications::create_api_key(
            &database,
            &app_id,
            &env_id,
            "Ingest",
            &key_hash,
            "sonde_ingest",
            &["telemetry.events".into()],
        )
        .await?;

        let now = chrono::Utc::now().timestamp_millis();
        let first = super::api_key_context(&database, &key_hash, now).await?;
        assert!(first.is_some());
        let second = super::api_key_context(&database, &key_hash, now + 1_000).await?;
        assert!(second.is_some());
        Ok(())
    }
}
