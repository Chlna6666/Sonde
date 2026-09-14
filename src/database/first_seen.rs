use std::collections::{BTreeMap, HashMap};

use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr, TransactionTrait,
    sea_query::{Alias, Condition, Expr, ExprTrait, Func, OnConflict, Order, Query, Value},
};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{
    query::insert_batch_ignore_conflicts,
    rollups::{self, DirtyDay},
};

const BACKFILL_STATE_KEY: &str = "telemetry_first_seen_backfill";
const BACKFILL_CURSOR_KEY: &str = "telemetry_first_seen_backfill_cursor";
const GLOBAL_SCOPE: &str = "global";
const APP_SCOPE: &str = "app";
const ENV_SCOPE: &str = "env";
const GLOBAL_VALUE: &str = "*";
const EXISTING_LOOKUP_CHUNK: usize = 500;
const BACKFILL_SEED_BATCH: u64 = 512;
const OLD_EPOCH_DELETE_BATCH: usize = 500;

#[derive(Clone, Debug)]
enum BackfillState {
    Seeding(String),
    Seeded(String),
    Complete(String),
}

impl BackfillState {
    fn epoch(&self) -> &str {
        match self {
            Self::Seeding(epoch) | Self::Seeded(epoch) | Self::Complete(epoch) => epoch,
        }
    }

    fn value(&self) -> String {
        match self {
            Self::Seeding(epoch) => format!("seeding:{epoch}"),
            Self::Seeded(epoch) => format!("seeded:{epoch}"),
            Self::Complete(epoch) => format!("complete:{epoch}"),
        }
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
struct SeedCursor {
    day: String,
    application_id: String,
    environment_id: String,
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

pub async fn run_backfill_batch(database: &DatabaseConnection, limit: u64) -> Result<usize, DbErr> {
    let state = ensure_backfill_seeded(database).await?;
    match state {
        BackfillState::Seeding(epoch) => {
            return seed_backfill_days_batch(database, &epoch, BACKFILL_SEED_BATCH).await;
        }
        BackfillState::Complete(epoch) => {
            return cleanup_other_epochs_batch(database, &epoch, OLD_EPOCH_DELETE_BATCH).await;
        }
        BackfillState::Seeded(epoch) => {
            let query = Query::select()
                .columns(["id", "application_id", "environment_id", "day"].map(Alias::new))
                .from(Alias::new("telemetry_first_seen_backfill_days"))
                .and_where(Expr::col(Alias::new("epoch")).eq(&epoch))
                .order_by(Alias::new("day"), Order::Asc)
                .order_by(Alias::new("application_id"), Order::Asc)
                .order_by(Alias::new("environment_id"), Order::Asc)
                .limit(std::cmp::max(limit, 1))
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
                processed = processed.saturating_add(
                    cleanup_other_epochs_batch(database, &epoch, OLD_EPOCH_DELETE_BATCH).await?,
                );
            }
            Ok(processed)
        }
    }
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
        &backfill_day_id(
            &epoch,
            &dirty.application_id,
            &dirty.environment_id,
            &dirty.day,
        ),
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

/// Invalidation is an O(1) epoch switch. Both the readiness state and seed cursor disappear
/// immediately, so all queries fall back to raw events and the next leased worker starts a new
/// epoch from the beginning. Old epoch rows are collected in bounded batches after the new epoch is
/// complete rather than under the retention/restore writer lock.
pub async fn invalidate(database: &DatabaseConnection) -> Result<(), DbErr> {
    let transaction = database.begin().await?;
    for key in [BACKFILL_STATE_KEY, BACKFILL_CURSOR_KEY] {
        let delete = Query::delete()
            .from_table(Alias::new("system_state"))
            .and_where(Expr::col(Alias::new("key")).eq(key))
            .to_owned();
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

    let clear_cursor = Query::delete()
        .from_table(Alias::new("system_state"))
        .and_where(Expr::col(Alias::new("key")).eq(BACKFILL_CURSOR_KEY))
        .to_owned();
    transaction.execute(&clear_cursor).await?;
    insert_batch_ignore_conflicts(
        &transaction,
        "system_state",
        &["key", "value"],
        vec![vec![
            Value::from(BACKFILL_STATE_KEY.to_owned()),
            Value::from(BackfillState::Seeding(epoch).value()),
        ]],
        "key",
        "key",
    )
    .await?;
    transaction.commit().await
}

async fn seed_backfill_days_batch(
    database: &DatabaseConnection,
    epoch: &str,
    limit: u64,
) -> Result<usize, DbErr> {
    let transaction = database.begin().await?;
    if !matches!(
        read_backfill_state(&transaction).await?,
        Some(BackfillState::Seeding(ref current)) if current == epoch
    ) {
        transaction.rollback().await?;
        return Ok(0);
    }

    let cursor = read_seed_cursor(&transaction).await?;
    let mut query = Query::select();
    query
        .columns(["application_id", "environment_id", "day"].map(Alias::new))
        .from(Alias::new("events"))
        .and_where(Expr::col(Alias::new("anonymous_id")).is_not_null())
        .distinct()
        .order_by(Alias::new("day"), Order::Asc)
        .order_by(Alias::new("application_id"), Order::Asc)
        .order_by(Alias::new("environment_id"), Order::Asc)
        .limit(std::cmp::max(limit, 1));
    if let Some(cursor) = &cursor {
        query.cond_where(
            Condition::any()
                .add(Expr::col(Alias::new("day")).gt(cursor.day.clone()))
                .add(
                    Condition::all()
                        .add(Expr::col(Alias::new("day")).eq(cursor.day.clone()))
                        .add(
                            Expr::col(Alias::new("application_id"))
                                .gt(cursor.application_id.clone()),
                        ),
                )
                .add(
                    Condition::all()
                        .add(Expr::col(Alias::new("day")).eq(cursor.day.clone()))
                        .add(
                            Expr::col(Alias::new("application_id"))
                                .eq(cursor.application_id.clone()),
                        )
                        .add(
                            Expr::col(Alias::new("environment_id"))
                                .gt(cursor.environment_id.clone()),
                        ),
                ),
        );
    }

    let scope_days = transaction
        .query_all(&query)
        .await?
        .into_iter()
        .map(|row| {
            Ok(SeedCursor {
                application_id: row.try_get("", "application_id")?,
                environment_id: row.try_get("", "environment_id")?,
                day: row.try_get("", "day")?,
            })
        })
        .collect::<Result<Vec<_>, DbErr>>()?;

    if scope_days.is_empty() {
        delete_seed_cursor(&transaction).await?;
        let has_pending = has_pending_backfill_on(&transaction, epoch).await?;
        let next = if has_pending {
            BackfillState::Seeded(epoch.to_owned())
        } else {
            BackfillState::Complete(epoch.to_owned())
        };
        if !transition_state_if_current(
            &transaction,
            &BackfillState::Seeding(epoch.to_owned()),
            &next,
        )
        .await?
        {
            transaction.rollback().await?;
            return Ok(0);
        }
        transaction.commit().await?;
        // A seeded epoch still has work to consume. Return a sentinel so callers that drain until
        // zero do not stop between the seeding and processing phases.
        return Ok(usize::from(has_pending));
    }

    let now = chrono::Utc::now().timestamp_millis();
    let mut rows = Vec::with_capacity(scope_days.len());
    for scope_day in &scope_days {
        rows.push(vec![
            Value::from(backfill_day_id(
                epoch,
                &scope_day.application_id,
                &scope_day.environment_id,
                &scope_day.day,
            )),
            Value::from(epoch.to_owned()),
            Value::from(scope_day.application_id.clone()),
            Value::from(scope_day.environment_id.clone()),
            Value::from(scope_day.day.clone()),
            Value::from(now),
        ]);
    }
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
    if let Some(last) = scope_days.last() {
        write_seed_cursor(&transaction, last).await?;
    }
    transaction.commit().await?;
    Ok(scope_days.len())
}

async fn read_seed_cursor(database: &impl ConnectionTrait) -> Result<Option<SeedCursor>, DbErr> {
    let query = Query::select()
        .column(Alias::new("value"))
        .from(Alias::new("system_state"))
        .and_where(Expr::col(Alias::new("key")).eq(BACKFILL_CURSOR_KEY))
        .limit(1)
        .to_owned();
    database
        .query_one(&query)
        .await?
        .map(|row| row.try_get::<String>("", "value"))
        .transpose()?
        .map(|value| {
            serde_json::from_str(&value)
                .map_err(|error| DbErr::Custom(format!("invalid first-seen seed cursor: {error}")))
        })
        .transpose()
}

async fn write_seed_cursor(
    database: &impl ConnectionTrait,
    cursor: &SeedCursor,
) -> Result<(), DbErr> {
    let value = serde_json::to_string(cursor).map_err(|error| {
        DbErr::Custom(format!("failed to encode first-seen seed cursor: {error}"))
    })?;
    let mut query = Query::insert();
    query
        .into_table(Alias::new("system_state"))
        .columns([Alias::new("key"), Alias::new("value")])
        .values(
            [
                Value::from(BACKFILL_CURSOR_KEY.to_owned()),
                Value::from(value),
            ]
            .into_iter()
            .map(Expr::value),
        )
        .map_err(|error| DbErr::Custom(error.to_string()))?
        .on_conflict(
            OnConflict::column(Alias::new("key"))
                .update_column(Alias::new("value"))
                .to_owned(),
        );
    database.execute(&query).await?;
    Ok(())
}

async fn delete_seed_cursor(database: &impl ConnectionTrait) -> Result<(), DbErr> {
    let delete = Query::delete()
        .from_table(Alias::new("system_state"))
        .and_where(Expr::col(Alias::new("key")).eq(BACKFILL_CURSOR_KEY))
        .to_owned();
    database.execute(&delete).await?;
    Ok(())
}

async fn transition_state_if_current(
    database: &impl ConnectionTrait,
    current: &BackfillState,
    next: &BackfillState,
) -> Result<bool, DbErr> {
    let update = Query::update()
        .table(Alias::new("system_state"))
        .value(Alias::new("value"), next.value())
        .and_where(Expr::col(Alias::new("key")).eq(BACKFILL_STATE_KEY))
        .and_where(Expr::col(Alias::new("value")).eq(current.value()))
        .to_owned();
    Ok(database.execute(&update).await?.rows_affected() == 1)
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
                .value(
                    Alias::new("first_seen_day"),
                    candidate.first_seen_day.clone(),
                )
                .value(Alias::new("updated_at"), now)
                .and_where(Expr::col(Alias::new("id")).eq(id))
                .and_where(Expr::col(Alias::new("epoch")).eq(epoch))
                .and_where(Expr::col(Alias::new("first_seen_at")).gt(candidate.first_seen_at))
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
    let total = row
        .and_then(|value| value.try_get::<i64>("", "total").ok())
        .unwrap_or(0);
    Ok(std::cmp::max(total, 0) as u64)
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
    let total = row
        .and_then(|value| value.try_get::<i64>("", "total").ok())
        .unwrap_or(0);
    Ok(std::cmp::max(total, 0) as u64)
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
        .and_where(rollups::dirty_source_condition(rollups::DIRTY_SOURCE_EVENT))
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
    if let Some(epoch) = value.strip_prefix("seeding:")
        && !epoch.is_empty()
    {
        return Ok(BackfillState::Seeding(epoch.to_owned()));
    }
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

async fn has_pending_backfill(database: &DatabaseConnection, epoch: &str) -> Result<bool, DbErr> {
    has_pending_backfill_on(database, epoch).await
}

async fn has_pending_backfill_on(
    database: &impl ConnectionTrait,
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
    transition_state_if_current(
        database,
        &BackfillState::Seeded(epoch.to_owned()),
        &BackfillState::Complete(epoch.to_owned()),
    )
    .await
}

async fn cleanup_other_epochs_batch(
    database: &DatabaseConnection,
    epoch: &str,
    limit: usize,
) -> Result<usize, DbErr> {
    let mut deleted = 0_usize;
    for table in [
        "telemetry_first_seen_backfill_days",
        "telemetry_user_first_seen",
    ] {
        if deleted >= limit {
            break;
        }
        let remaining = limit.saturating_sub(deleted);
        let select = Query::select()
            .column(Alias::new("id"))
            .from(Alias::new(table))
            .and_where(Expr::col(Alias::new("epoch")).ne(epoch))
            .limit(remaining as u64)
            .to_owned();
        let ids = database
            .query_all(&select)
            .await?
            .into_iter()
            .filter_map(|row| row.try_get::<String>("", "id").ok())
            .collect::<Vec<_>>();
        if ids.is_empty() {
            continue;
        }
        let delete = Query::delete()
            .from_table(Alias::new(table))
            .and_where(Expr::col(Alias::new("id")).is_in(ids))
            .to_owned();
        deleted = deleted.saturating_add(database.execute(&delete).await?.rows_affected() as usize);
    }
    Ok(deleted)
}

async fn delete_pending_day(database: &impl ConnectionTrait, id: &str) -> Result<(), DbErr> {
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
    hasher.update(b"sonde:first-seen\0");
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
    hasher.update(b"sonde:first-seen\0backfill\0");
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
