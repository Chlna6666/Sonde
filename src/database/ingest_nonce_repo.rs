use sea_orm::{ConnectionTrait, DatabaseConnection, DbErr, SqlErr, sea_query::{Alias, Expr, ExprTrait, Query}};
use sha2::{Digest, Sha256};

use super::query;

const REPLAY_KEY_CONTEXT: &[u8] = b"sonde-ingest-nonce-replay-v1\n";

/// Atomically record a token/nonce pair.
///
/// The database unique key is the authority for replay protection across replicas and process
/// restarts. Only a one-way digest is persisted; raw token ids and nonces are not stored.
pub async fn record_once(
    database: &DatabaseConnection,
    token_id: &str,
    nonce: &str,
    expires_at: i64,
) -> Result<bool, DbErr> {
    let now = chrono::Utc::now().timestamp_millis();
    let replay_key = replay_key(token_id, nonce);
    match query::insert(
        database,
        "ingest_nonce_replay",
        &["replay_key", "expires_at", "created_at"],
        vec![replay_key.into(), expires_at.into(), now.into()],
    )
    .await
    {
        Ok(_) => Ok(true),
        Err(error) if matches!(error.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) => {
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

pub async fn cleanup_expired(
    database: &impl ConnectionTrait,
    now: i64,
) -> Result<u64, DbErr> {
    let delete = Query::delete()
        .from_table(Alias::new("ingest_nonce_replay"))
        .and_where(Expr::col(Alias::new("expires_at")).lte(now))
        .to_owned();
    Ok(database.execute(&delete).await?.rows_affected())
}

fn replay_key(token_id: &str, nonce: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(REPLAY_KEY_CONTEXT);
    hasher.update(token_id.as_bytes());
    hasher.update(b"\n");
    hasher.update(nonce.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::replay_key;

    #[test]
    fn replay_key_is_stable_and_separates_token_nonce_pairs() {
        let key = replay_key("token-a", "nonce-a");
        assert_eq!(key.len(), 64);
        assert_eq!(key, replay_key("token-a", "nonce-a"));
        assert_ne!(key, replay_key("token-b", "nonce-a"));
        assert_ne!(key, replay_key("token-a", "nonce-b"));
    }
}
