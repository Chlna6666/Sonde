use std::collections::{BTreeMap, HashMap};

use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr, TransactionTrait,
    sea_query::{Alias, Expr, ExprTrait, Func, Order, Query, Value},
};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{query::insert_batch_ignore_conflicts, rollup_repo::DirtyDay};

const BACKFILL_STATE_KEY: &str = "telemetry_first_seen_backfill_v1";
const GLOBAL_SCOPE: &str = "global";
const APP_SCOPE: &str = "app";
const ENV_SCOPE: &str = "env";
const GLOBAL_VALUE: &str = "*";
const EXISTING_LOOKUP_CHUNK: usize = 500;

#[derive(Clone, Debug)]
enum BackfillState {
    Seeded(String),
    Complete(String),
}

impl BackfillState {
    fn epoch(&self) -> &str {
        match self {
            Self::Seeded(epoch) | Self::Complete(epoch) => epoch,
        }
    }

    fn value(&self) -> String {
        match self {
            Self::Seeded(epoch) => format!("seeded:{epoch}"),
            Self::Complete(epoch) => format!("complete:{epoch}"),
        }
    }
}

#[derive(Debug)]
struct FirstSeenCandidate {
    id: String,
    scope_kind: &'static str,
    application_id: String,
    environment_id: String,
    first_seen_at: i64,
    first_seen_day: String,
}

#[derive(Debug)]
struct BackfillDay {
    id: String,
    application_id: String,
    environment_id: String,
    day: String,
}

pub async fn run_backfill_batch(
    database: &DatabaseConnection,
    limit: u64,
) -> Result<usize, DbErr> {
    let state = ensure_backfill_seeded(database).await?;
    let BackfillState::Seeded(epoch) = state else {
        return Ok(0);
    };

    let query = Query::select()
        .columns(["id", "application_id", "environment_id", "day"].map(Alias::new))
        .from(Alias::new("telemetry_first_seen_backfill_days"))
        .and_where(Expr::col(Alias::new("epoch")).eq(&epoch))
        .order_by(Alias::new("day"), Order::Asc)
        .order_by(Alias::new("application_id"), Order::Asc)
        .order_by(Alias::new("environment_id"), Order::Asc)
        .limit(limit.max(1))
        .to_owned();
    let days = database
        .query_all(&query)
        .await?
        .into_iter()
        .map(|row| {
            Ok(BackfillDay {
                id: row.try_get("", "id")?,
                application_id: row.try_get("", "application_id")?,
                environment_id: row.try_get("", "environment_id")?,
                day: row.try_get("", "day")?,
            })
        })
        .collect::<Result<Vec<_>, DbErr>>()?;

    let mut processed = 0_usize;
    for day in &days {
        if !recompute_backfill_day(database, day, &epoch).await? {
            break;
        }
        processed = processed.saturating_add(1);
        tokio::task::yield_now().await;
    }

    if !has_pending_backfill(database, &epoch).await?
        && mark_complete_if_current(database, &epoch).await?
    {
        cleanup_other_epochs(database, &epoch).await?;
    }
    Ok(processed)
}

pub async fn refresh_dirty_day(
    database: &DatabaseConnection,
    dirty: &DirtyDay,
) -> Result<bool, DbErr> {
    if dirty.environment_id == GLOBAL_VALUE {
        return Ok(true);
    }
    let Some(state) = read_backfill_state(database).await? else {
        return Ok(false);
    };
    let epoch = state.epoch().to_owned();

    let transaction = database.begin().await?;
    let marker = Query::select()
        .column(Alias::new("generation"))
        .from(Alias::new("telemetry_dirty_days"))
        .and_where(Expr::col(Alias::new("id")).eq(&dirty.id))
        .limit(1)
        .to_owned();
    let generation = transaction
        .query_one(&marker)
        .await?
        .and_then(|row| row.try_get::<i64>("", "generation").ok());
    if generation != Some(dirty.generation)
        || !backfill_epoch_is_current(&transaction, &epoch).await?
    {
        transaction.rollback().await?;
        return Ok(false);
    }

    recompute_scope_day(
        &transaction,
        &dirty.application_id,
        &dirty.environment_id,
        &dirty.day,
        &epoch,
    )
    .await?;
    delete_pending_day(
        &transaction,
        &backfill_day_id(&epoch, &dirty.application_id, &dirty.environment_id, &dirty.day),
    )
    .await?;
    transaction.commit().await?;
    Ok(true)
}

pub async fn count_new_users_hybrid(
    database: &DatabaseConnection,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since: i64,
    until: Option<i64>,
) -> Result<u64, DbErr> {
    if until.is_some_and(|end| end <= since) {
        return Ok(0);
    }

    if let Some(BackfillState::Complete(epoch)) = read_backfill_state(database).await?
        && !relevant_dirty_exists(database, application_id, environment_id, until).await?
    {
        return count_indexed_new_users(
            database,
            &epoch,
            application_id,
            environment_id,
            since,
            until,
        )
        .await;
    }
    raw_new_users(database, application_id, environment_id, since, until).await
}

pub async fn invalidate(database: &DatabaseConnection) -> Result<(), DbErr> {
    let transaction = database.begin().await?;
    let clear_state = Query::delete()
        .from_table(Alias::new("system_state"))
        .and_where(Expr::col(Alias::new("key")).eq(BACKFILL_STATE_KEY))
        .to_owned();
    transaction.execute(&clear_state).await?;
    for table in [
        "telemetry_first_seen_backfill_days",
        "telemetry_user_first_seen",
    ] {
        let delete = Query::delete().from_table(Alias::new(table)).to_owned();
        transaction.execute(&delete).await?;
    }
    transaction.commit().await
}

pub async fn backfill_complete(database: &DatabaseConnection) -> Result<bool, DbErr> {
    Ok(matches!(
        read_backfill_state(database).await?,
        Some(BackfillState::Complete(_))
    ))
}

async fn ensure_backfill_seeded(database: &DatabaseConnection) -> Result<BackfillState, DbErr> {
    loop {
        if let Some(state) = read_backfill_state(database).await? {
            return Ok(state);
        }
        seed_new_epoch(database).await?;
        if let Some(state) = read_backfill_state(database).await? {
            return Ok(state);
        }
        tokio::task::yield_now().await;
    }
}

async fn seed_new_epoch(database: &DatabaseConnection) -> Result<(), DbErr> {
    let epoch = Uuid::now_v7().to_string();
    let transaction = database.begin().await?;
    if read_backfill_state(&transaction).await?.is_some() {
        transaction.rollback().await?;
        return Ok(());
    }

    for table in [
        "telemetry_first_seen_backfill_days",
        "telemetry_user_first_seen",
    ] {
        let delete = Query::delete().from_table(Alias::new(table)).to_owned();
        transaction.execute(&delete).await?;
    }

    let query = Query::select()
        .columns(["application_id", "environment_id", "day"].map(Alias::new))
        .from(Alias::new("events"))
        .and_where(Expr::col(Alias::new("anonymous_id")).is_not_null())
        .distinct()
        .to_owned();
    let now = chrono::Utc::now().timestamp_millis();
    let mut rows = Vec::new();
    for row in transaction.query_all(&query).await? {
        let application_id: String = row.try_get("", "application_id")?;
        let environment_id: String = row.try_get("", "environment_id")?;
        let day: String = row.try_get("", "day")?;
        rows.push(vec![
            Value::from(backfill_day_id(
                &epoch,
                &application_id,
                &environment_id,
                &day,
            )),
            Value::from(epoch.clone()),
            Value::from(application_id),
            Value::from(environment_id),
            Value::from(day),
            Value::from(now),
        ]);
    }
    let has_pending = !rows.is_empty();
    insert_batch_ignore_conflicts(
        &transaction,
        "telemetry_first_seen_backfill_days",
        &[
            "id",
            "epoch",
            "application_id",
            "environment_id",
            "day",
            "created_at",
        ],
        rows,
        "id",
        "id",
    )
    .await?;
    let state = if has_pending {
        BackfillState::Seeded(epoch)
    } else {
        BackfillState::Complete(epoch)
    };
    insert_batch_ignore_conflicts(
        &transaction,
        "system_state",
        &["key", "value"],
        vec![vec![
            Value::from(BACKFILL_STATE_KEY.to_owned()),
            Value::from(state.value()),
        ]],
        "key",
        "key",
    )
    .await?;
    transaction.commit().await
}

async fn recompute_backfill_day(
    database: &DatabaseConnection,
    day: &BackfillDay,
    epoch: &str,
) -> Result<bool, DbErr> {
    let transaction = database.begin().await?;
    if !backfill_epoch_is_current(&transaction, epoch).await? {
        transaction.rollback().await?;
        return Ok(false);
    }
    recompute_scope_day(
        &transaction,
        &day.application_id,
        &day.environment_id,
        &day.day,
        epoch,
    )
    .await?;
    delete_pending_day(&transaction, &day.id).await?;
    transaction.commit().await?;
    Ok(true)
}

async fn recompute_scope_day(
    database: &impl ConnectionTrait,
    application_id: &str,
    environment_id: &str,
    day: &str,
    epoch: &str,
) -> Result<(), DbErr> {
    let query = Query::select()
        .column(Alias::new("anonymous_id"))
        .expr_as(
            Func::min(Expr::col(Alias::new("timestamp"))),
            Alias::new("first_seen_at"),
        )
        .from(Alias::new("events"))
        .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
        .and_where(Expr::col(Alias::new("environment_id")).eq(environment_id))
        .and_where(Expr::col(Alias::new("day")).eq(day))
        .and_where(Expr::col(Alias::new("anonymous_id")).is_not_null())
        .group_by_col(Alias::new("anonymous_id"))
        .to_owned();

    let mut candidates = BTreeMap::<String, FirstSeenCandidate>::new();
    for row in database.query_all(&query).await? {
        let anonymous_id: String = row.try_get("", "anonymous_id")?;
        let first_seen_at: i64 = row.try_get("", "first_seen_at")?;
        let first_seen_day = day_for_timestamp(first_seen_at).unwrap_or_else(|| day.to_owned());
        for (scope_kind, scope_app, scope_env) in [
            (ENV_SCOPE, application_id, environment_id),
            (APP_SCOPE, application_id, GLOBAL_VALUE),
            (GLOBAL_SCOPE, GLOBAL_VALUE, GLOBAL_VALUE),
        ] {
            let id = first_seen_id(epoch, scope_kind, scope_app, scope_env, &anonymous_id);
            let candidate = FirstSeenCandidate {
                id: id.clone(),
                scope_kind,
                application_id: scope_app.to_owned(),
                environment_id: scope_env.to_owned(),
                first_seen_at,
                first_seen_day: first_seen_day.clone(),
            };
            candidates
                .entry(id)
                .and_modify(|current| {
                    if candidate.first_seen_at < current.first_seen_at {
                        current.first_seen_at = candidate.first_seen_at;
                        current.first_seen_day = candidate.first_seen_day.clone();
                    }
                })
                .or_insert(candidate);
        }
    }
    if candidates.is_empty() {
        return Ok(());
    }

    let now = chrono::Utc::now().timestamp_millis();
    let insert_rows = candidates
        .values()
        .map(|candidate| {
            vec![
                Value::from(candidate.id.clone()),
                Value::from(epoch.to_owned()),
                Value::from(candidate.scope_kind.to_owned()),
                Value::from(candidate.application_id.clone()),
                Value::from(candidate.environment_id.clone()),
                Value::from(candidate.first_seen_at),
                Value::from(candidate.first_seen_day.clone()),
                Value::from(now),
            ]
        })
        .collect();
    insert_batch_ignore_conflicts(
        database,
        "telemetry_user_first_seen",
        &[
            "id",
            "epoch",
            "scope_kind",
            "application_id",
            "environment_id",
            "first_seen_at",
            "first_seen_day",
            "updated_at",
        ],
        insert_rows,
        "id",
        "id",
    )
    .await?;

    let ids = candidates.keys().cloned().collect::<Vec<_>>();
    for chunk in ids.chunks(EXISTING_LOOKUP_CHUNK) {
        let lookup = Query::select()
            .columns(["id", "first_seen_at"].map(Alias::new))
            .from(Alias::new("telemetry_user_first_seen"))
            .and_where(Expr::col(Alias::new("id")).is_in(chunk.iter().cloned()))
            .to_owned();
        let current = database
            .query_all(&lookup)
            .await?
            .into_iter()
            .filter_map(|row| {
                Some((
                    row.try_get::<String>("", "id").ok()?,
                    row.try_get::<i64>("", "first_seen_at").ok()?,
                ))
            })
            .collect::<HashMap<_, _>>();
        for id in chunk {
            let Some(candidate) = candidates.get(id) else {
                continue;
            };
            if current
                .get(id)
                .is_none_or(|stored| candidate.first_seen_at >= *stored)
            {
                continue;
            }
            let update = Query::update()
                .table(Alias::new("telemetry_user_first_seen"))
                .value(Alias::new("first_seen_at"), candidate.first_seen_at)
                .value(Alias::new("first_seen_day"), candidate.first_seen_day.clone())
                .value(Alias::new("updated_at"), now)
                .and_where(Expr::col(Alias::new("id")).eq(id))
                .and_where(Expr::col(Alias::new("epoch")).eq(epoch))
                .and_where(
                    Expr::col(Alias::new("first_seen_at")).gt(candidate.first_seen_at),
                )
                .to_owned();
            database.execute(&update).await?;
        }
    }
    Ok(())
}

async fn count_indexed_new_users(
    database: &DatabaseConnection,
    epoch: &str,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since: i64,
    until: Option<i64>,
) -> Result<u64, DbErr> {
    let (scope_kind, scope_app, scope_env) = scope(application_id, environment_id)?;
    let mut query = Query::select();
    query
        .expr_as(
            Func::count(Expr::col(Alias::new("id"))),
            Alias::new("total"),
        )
        .from(Alias::new("telemetry_user_first_seen"))
        .and_where(Expr::col(Alias::new("epoch")).eq(epoch))
        .and_where(Expr::col(Alias::new("scope_kind")).eq(scope_kind))
        .and_where(Expr::col(Alias::new("application_id")).eq(scope_app))
        .and_where(Expr::col(Alias::new("environment_id")).eq(scope_env))
        .and_where(Expr::col(Alias::new("first_seen_at")).gte(since));
    if let Some(until) = until {
        query.and_where(Expr::col(Alias::new("first_seen_at")).lt(until));
    }
    let row = database.query_one(&query).await?;
    Ok(row
        .and_then(|value| value.try_get::<i64>("", "total").ok())
        .unwrap_or(0)
        .max(0) as u64)
}

async fn raw_new_users(
    database: &DatabaseConnection,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since: i64,
    until: Option<i64>,
) -> Result<u64, DbErr> {
    let mut first_seen = Query::select();
    first_seen
        .column(Alias::new("anonymous_id"))
        .expr_as(
            Func::min(Expr::col(Alias::new("timestamp"))),
            Alias::new("first_seen_at"),
        )
        .from(Alias::new("events"))
        .and_where(Expr::col(Alias::new("anonymous_id")).is_not_null());
    if let Some(application_id) = application_id {
        first_seen.and_where(Expr::col(Alias::new("application_id")).eq(application_id));
    }
    if let Some(environment_id) = environment_id {
        first_seen.and_where(Expr::col(Alias::new("environment_id")).eq(environment_id));
    }
    first_seen.group_by_col(Alias::new("anonymous_id"));

    let mut query = Query::select();
    query
        .expr_as(
            Func::count(Expr::col(Alias::new("anonymous_id"))),
            Alias::new("total"),
        )
        .from_subquery(first_seen.take(), Alias::new("first_seen"))
        .and_where(Expr::col(Alias::new("first_seen_at")).gte(since));
    if let Some(until) = until {
        query.and_where(Expr::col(Alias::new("first_seen_at")).lt(until));
    }
    let row = database.query_one(&query).await?;
    Ok(row
        .and_then(|value| value.try_get::<i64>("", "total").ok())
        .unwrap_or(0)
        .max(0) as u64)
}

async fn relevant_dirty_exists(
    database: &DatabaseConnection,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    until: Option<i64>,
) -> Result<bool, DbErr> {
    let mut query = Query::select();
    query
        .column(Alias::new("id"))
        .from(Alias::new("telemetry_dirty_days"))
        .limit(1);
    if let Some(application_id) = application_id {
        query.and_where(Expr::col(Alias::new("application_id")).eq(application_id));
    }
    if let Some(environment_id) = environment_id {
        query.and_where(Expr::col(Alias::new("environment_id")).eq(environment_id));
    }
    if let Some(until) = until
        && let Some(day) = until.checked_sub(1).and_then(day_for_timestamp)
    {
        query.and_where(Expr::col(Alias::new("day")).lte(day));
    }
    Ok(database.query_one(&query).await?.is_some())
}

async fn read_backfill_state(
    database: &impl ConnectionTrait,
) -> Result<Option<BackfillState>, DbErr> {
    let query = Query::select()
        .column(Alias::new("value"))
        .from(Alias::new("system_state"))
        .and_where(Expr::col(Alias::new("key")).eq(BACKFILL_STATE_KEY))
        .limit(1)
        .to_owned();
    database
        .query_one(&query)
        .await?
        .map(|row| row.try_get::<String>("", "value"))
        .transpose()?
        .map(|value| parse_backfill_state(&value))
        .transpose()
}

fn parse_backfill_state(value: &str) -> Result<BackfillState, DbErr> {
    if let Some(epoch) = value.strip_prefix("seeded:")
        && !epoch.is_empty()
    {
        return Ok(BackfillState::Seeded(epoch.to_owned()));
    }
    if let Some(epoch) = value.strip_prefix("complete:")
        && !epoch.is_empty()
    {
        return Ok(BackfillState::Complete(epoch.to_owned()));
    }
    Err(DbErr::Custom("invalid first-seen backfill state".into()))
}

async fn backfill_epoch_is_current(
    database: &impl ConnectionTrait,
    epoch: &str,
) -> Result<bool, DbErr> {
    Ok(read_backfill_state(database)
        .await?
        .is_some_and(|state| state.epoch() == epoch))
}

async fn has_pending_backfill(
    database: &DatabaseConnection,
    epoch: &str,
) -> Result<bool, DbErr> {
    let query = Query::select()
        .column(Alias::new("id"))
        .from(Alias::new("telemetry_first_seen_backfill_days"))
        .and_where(Expr::col(Alias::new("epoch")).eq(epoch))
        .limit(1)
        .to_owned();
    Ok(database.query_one(&query).await?.is_some())
}

async fn mark_complete_if_current(
    database: &DatabaseConnection,
    epoch: &str,
) -> Result<bool, DbErr> {
    let seeded = BackfillState::Seeded(epoch.to_owned()).value();
    let complete = BackfillState::Complete(epoch.to_owned()).value();
    let update = Query::update()
        .table(Alias::new("system_state"))
        .value(Alias::new("value"), complete)
        .and_where(Expr::col(Alias::new("key")).eq(BACKFILL_STATE_KEY))
        .and_where(Expr::col(Alias::new("value")).eq(seeded))
        .to_owned();
    Ok(database.execute(&update).await?.rows_affected() == 1)
}

async fn cleanup_other_epochs(
    database: &DatabaseConnection,
    epoch: &str,
) -> Result<(), DbErr> {
    for table in [
        "telemetry_first_seen_backfill_days",
        "telemetry_user_first_seen",
    ] {
        let delete = Query::delete()
            .from_table(Alias::new(table))
            .and_where(Expr::col(Alias::new("epoch")).ne(epoch))
            .to_owned();
        database.execute(&delete).await?;
    }
    Ok(())
}

async fn delete_pending_day(
    database: &impl ConnectionTrait,
    id: &str,
) -> Result<(), DbErr> {
    let delete = Query::delete()
        .from_table(Alias::new("telemetry_first_seen_backfill_days"))
        .and_where(Expr::col(Alias::new("id")).eq(id))
        .to_owned();
    database.execute(&delete).await?;
    Ok(())
}

fn scope<'a>(
    application_id: Option<&'a str>,
    environment_id: Option<&'a str>,
) -> Result<(&'static str, &'a str, &'a str), DbErr> {
    match (application_id, environment_id) {
        (None, None) => Ok((GLOBAL_SCOPE, GLOBAL_VALUE, GLOBAL_VALUE)),
        (Some(application_id), None) => Ok((APP_SCOPE, application_id, GLOBAL_VALUE)),
        (Some(application_id), Some(environment_id)) => {
            Ok((ENV_SCOPE, application_id, environment_id))
        }
        (None, Some(_)) => Err(DbErr::Custom(
            "environment-scoped first-seen query requires application id".into(),
        )),
    }
}

fn first_seen_id(
    epoch: &str,
    scope_kind: &str,
    application_id: &str,
    environment_id: &str,
    anonymous_id: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"sonde:first-seen:v2\0");
    hasher.update(epoch.as_bytes());
    hasher.update(b"\0");
    hasher.update(scope_kind.as_bytes());
    hasher.update(b"\0");
    hasher.update(application_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(environment_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(anonymous_id.as_bytes());
    format!("fs_{}", hex::encode(hasher.finalize()))
}

fn backfill_day_id(epoch: &str, application_id: &str, environment_id: &str, day: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"sonde:first-seen:v2\0backfill\0");
    hasher.update(epoch.as_bytes());
    hasher.update(b"\0");
    hasher.update(application_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(environment_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(day.as_bytes());
    format!("fsb_{}", hex::encode(hasher.finalize()))
}

fn day_for_timestamp(timestamp: i64) -> Option<String> {
    chrono::DateTime::from_timestamp_millis(timestamp)
        .map(|value| value.format("%Y-%m-%d").to_string())
}
