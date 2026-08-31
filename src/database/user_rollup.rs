use std::collections::{BTreeMap, BTreeSet};

use chrono::NaiveDate;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr, TransactionTrait,
    sea_query::{Alias, Expr, ExprTrait, OnConflict, Order, Query, Value},
};
use sha2::{Digest, Sha256};

use super::{
    rollup_repo::{self, DirtyDay},
    telemetry_repo::TelemetryScope,
};

const GLOBAL_ENVIRONMENT: &str = "*";
const USER_ROLLUP_BACKFILL_KEY: &str = "telemetry_user_rollup_backfill_v1";
const FINGERPRINT_BYTES: usize = 16;
const FINGERPRINTS_PER_CHUNK: usize = 2_048;
const USER_SET_INSERT_CHUNK: usize = 100;
const MAX_DIRTY_SCOPE_DAYS: usize = 32;

type Fingerprint = [u8; FINGERPRINT_BYTES];

struct ScopedUserSet {
    application_id: String,
    day: String,
    fingerprints: Vec<Fingerprint>,
}

struct DailyUserSet {
    day: String,
    fingerprints: Vec<Fingerprint>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserGrowthBucket {
    pub bucket: String,
    pub new_users: u64,
    pub cumulative_users: u64,
    pub active_users: u64,
}

pub async fn seed_historical_user_dirty_days_once(
    database: &DatabaseConnection,
) -> Result<usize, DbErr> {
    if user_backfill_seeded(database).await? {
        return Ok(0);
    }

    let query = Query::select()
        .columns(["application_id", "environment_id", "day"].map(Alias::new))
        .from(Alias::new("events"))
        .and_where(Expr::col(Alias::new("anonymous_id")).is_not_null())
        .distinct()
        .to_owned();
    let mut scopes = BTreeMap::<(String, String), Vec<i64>>::new();
    let mut scope_days = 0_usize;
    for row in database.query_all(&query).await? {
        let application_id: String = row.try_get("", "application_id")?;
        let environment_id: String = row.try_get("", "environment_id")?;
        let day: String = row.try_get("", "day")?;
        let Some(timestamp) = day_start_timestamp(&day) else {
            continue;
        };
        scopes
            .entry((application_id, environment_id))
            .or_default()
            .push(timestamp);
        scope_days = scope_days.saturating_add(1);
    }

    for ((application_id, environment_id), timestamps) in scopes {
        let scope = TelemetryScope {
            application_id,
            environment_id,
        };
        rollup_repo::mark_dirty_timestamps_for_source(
            database,
            &scope,
            rollup_repo::DIRTY_SOURCE_EVENT,
            timestamps,
        )
        .await?;
    }
    set_system_state(database, USER_ROLLUP_BACKFILL_KEY, "complete").await?;
    Ok(scope_days)
}

pub async fn user_backfill_seeded(database: &DatabaseConnection) -> Result<bool, DbErr> {
    let query = Query::select()
        .column(Alias::new("value"))
        .from(Alias::new("system_state"))
        .and_where(Expr::col(Alias::new("key")).eq(USER_ROLLUP_BACKFILL_KEY))
        .limit(1)
        .to_owned();
    Ok(database
        .query_one(&query)
        .await?
        .and_then(|row| row.try_get::<String>("", "value").ok())
        .is_some_and(|value| value == "complete"))
}

pub async fn invalidate_user_backfill(database: &DatabaseConnection) -> Result<(), DbErr> {
    let delete = Query::delete()
        .from_table(Alias::new("system_state"))
        .and_where(Expr::col(Alias::new("key")).eq(USER_ROLLUP_BACKFILL_KEY))
        .to_owned();
    database.execute(&delete).await?;
    Ok(())
}

pub async fn recompute_claimed_day_user_set(
    database: &DatabaseConnection,
    dirty: &DirtyDay,
) -> Result<bool, DbErr> {
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
    if generation != Some(dirty.generation) {
        transaction.rollback().await?;
        return Ok(false);
    }

    let environment = (dirty.environment_id != GLOBAL_ENVIRONMENT)
        .then_some(dirty.environment_id.as_str());
    let fingerprints = raw_fingerprints(
        &transaction,
        Some(&dirty.application_id),
        environment,
        None,
        None,
        Some(&dirty.day),
    )
    .await?;

    let delete_existing = Query::delete()
        .from_table(Alias::new("telemetry_daily_user_sets"))
        .and_where(Expr::col(Alias::new("application_id")).eq(&dirty.application_id))
        .and_where(Expr::col(Alias::new("environment_id")).eq(&dirty.environment_id))
        .and_where(Expr::col(Alias::new("day")).eq(&dirty.day))
        .to_owned();
    transaction.execute(&delete_existing).await?;

    let now = chrono::Utc::now().timestamp_millis();
    let mut rows = Vec::with_capacity(fingerprints.len().div_ceil(FINGERPRINTS_PER_CHUNK));
    for (chunk_index, chunk) in fingerprints.chunks(FINGERPRINTS_PER_CHUNK).enumerate() {
        let chunk_index = i32::try_from(chunk_index)
            .map_err(|_| DbErr::Custom("daily user-set chunk index overflow".into()))?;
        rows.push(vec![
            Value::from(user_set_id(
                &dirty.application_id,
                &dirty.environment_id,
                &dirty.day,
                chunk_index,
            )),
            Value::from(dirty.application_id.clone()),
            Value::from(dirty.environment_id.clone()),
            Value::from(dirty.day.clone()),
            Value::from(chunk_index),
            Value::from(saturating_i64(chunk.len() as u64)),
            Value::from(encode_fingerprints(chunk)),
            Value::from(now),
        ]);
    }
    insert_user_set_rows(&transaction, rows).await?;

    transaction.commit().await?;
    Ok(true)
}

/// Returns the unique anonymous-user count in `[since, until)`.
///
/// Clean complete days are served from compact sorted fingerprint chunks. Dirty days and partial
/// boundary days are recomputed from authoritative raw events. A large dirty backlog falls back to
/// one raw DISTINCT query instead of issuing one query per dirty day.
pub async fn unique_users_hybrid(
    database: &DatabaseConnection,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since: Option<i64>,
    until: Option<i64>,
) -> Result<u64, DbErr> {
    if matches!((since, until), (Some(start), Some(end)) if end <= start) {
        return Ok(0);
    }

    if let Some(daily_sets) = load_daily_user_sets_hybrid(
        database,
        application_id,
        environment_id,
        since,
        until,
    )
    .await?
    {
        let sets = daily_sets
            .into_iter()
            .map(|value| value.fingerprints)
            .collect::<Vec<_>>();
        return Ok(union_sorted_sets(sets).len() as u64);
    }

    Ok(raw_fingerprints(database, application_id, environment_id, since, until, None)
        .await?
        .len() as u64)
}

/// Projects the same hybrid daily user sets into daily or monthly growth buckets.
///
/// `monthly=false` matches normal multi-day statistics windows. `monthly=true` matches the existing
/// 365-day/all-time month bucketing. Hourly one-day statistics deliberately remain on the raw path.
pub async fn user_growth_hybrid(
    database: &DatabaseConnection,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since: Option<i64>,
    monthly: bool,
) -> Result<Option<Vec<UserGrowthBucket>>, DbErr> {
    let Some(daily_sets) = load_daily_user_sets_hybrid(
        database,
        application_id,
        environment_id,
        since,
        None,
    )
    .await?
    else {
        return Ok(None);
    };

    let mut grouped = BTreeMap::<String, Vec<Vec<Fingerprint>>>::new();
    for daily in daily_sets {
        let bucket = if monthly {
            daily
                .day
                .get(..7)
                .ok_or_else(|| DbErr::Custom("invalid daily user-set day".into()))?
                .to_owned()
        } else {
            daily.day
        };
        grouped.entry(bucket).or_default().push(daily.fingerprints);
    }

    let mut cumulative = Vec::<Fingerprint>::new();
    let mut result = Vec::with_capacity(grouped.len());
    for (bucket, sets) in grouped {
        let active = union_sorted_sets(sets);
        let active_users = active.len() as u64;
        let (next_cumulative, new_users) = merge_sorted_unique_count_new(cumulative, active);
        cumulative = next_cumulative;
        result.push(UserGrowthBucket {
            bucket,
            new_users: new_users as u64,
            cumulative_users: cumulative.len() as u64,
            active_users,
        });
    }
    Ok(Some(result))
}

async fn load_daily_user_sets_hybrid(
    database: &DatabaseConnection,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since: Option<i64>,
    until: Option<i64>,
) -> Result<Option<Vec<DailyUserSet>>, DbErr> {
    if !user_backfill_seeded(database).await? {
        return Ok(None);
    }

    let since_day = since.and_then(day_for_timestamp);
    let until_day = until
        .and_then(|value| value.checked_sub(1))
        .and_then(day_for_timestamp);
    if since.is_some() && since_day.is_none() || until.is_some() && until_day.is_none() {
        return Ok(None);
    }

    let rollup_environment = environment_id.unwrap_or(GLOBAL_ENVIRONMENT);
    let mut query = Query::select();
    query
        .columns(
            [
                "application_id",
                "day",
                "chunk_index",
                "user_count",
                "fingerprints",
            ]
            .map(Alias::new),
        )
        .from(Alias::new("telemetry_daily_user_sets"))
        .and_where(Expr::col(Alias::new("environment_id")).eq(rollup_environment))
        .order_by(Alias::new("day"), Order::Asc)
        .order_by(Alias::new("chunk_index"), Order::Asc);
    if let Some(application_id) = application_id {
        query.and_where(Expr::col(Alias::new("application_id")).eq(application_id));
    }
    if let Some(day) = since_day.as_deref() {
        query.and_where(Expr::col(Alias::new("day")).gte(day));
    }
    if let Some(day) = until_day.as_deref() {
        query.and_where(Expr::col(Alias::new("day")).lte(day));
    }

    let mut sets = Vec::<ScopedUserSet>::new();
    for row in database.query_all(&query).await? {
        let blob: Vec<u8> = row.try_get("", "fingerprints")?;
        let fingerprints = decode_fingerprints(&blob)?;
        let declared = positive_u64(row.try_get::<i64>("", "user_count").unwrap_or(0));
        if declared != fingerprints.len() as u64 {
            return Err(DbErr::Custom("daily user-set fingerprint count mismatch".into()));
        }
        if fingerprints.len() > FINGERPRINTS_PER_CHUNK {
            return Err(DbErr::Custom("daily user-set chunk exceeds configured size".into()));
        }
        sets.push(ScopedUserSet {
            application_id: row.try_get("", "application_id")?,
            day: row.try_get("", "day")?,
            fingerprints,
        });
    }

    let mut dirty_query = Query::select();
    dirty_query
        .columns(["application_id", "day"].map(Alias::new))
        .from(Alias::new("telemetry_dirty_days"))
        .and_where(Expr::col(Alias::new("environment_id")).eq(rollup_environment))
        .and_where(rollup_repo::dirty_source_condition(
            rollup_repo::DIRTY_SOURCE_EVENT,
        ));
    if let Some(application_id) = application_id {
        dirty_query.and_where(Expr::col(Alias::new("application_id")).eq(application_id));
    }
    if let Some(day) = since_day.as_deref() {
        dirty_query.and_where(Expr::col(Alias::new("day")).gte(day));
    }
    if let Some(day) = until_day.as_deref() {
        dirty_query.and_where(Expr::col(Alias::new("day")).lte(day));
    }
    let mut dirty = database
        .query_all(&dirty_query)
        .await?
        .into_iter()
        .filter_map(|row| {
            Some((
                row.try_get::<String>("", "application_id").ok()?,
                row.try_get::<String>("", "day").ok()?,
            ))
        })
        .collect::<BTreeSet<_>>();

    let mut fallback_sets = Vec::<ScopedUserSet>::new();
    for day in partial_boundary_days(since, until)? {
        sets.retain(|stored| stored.day != day);
        dirty.retain(|(_, dirty_day)| dirty_day != &day);
        let (day_start, day_end) = day_bounds(&day)?;
        let range_start = since.map_or(day_start, |value| value.max(day_start));
        let range_end = until.map_or(day_end, |value| value.min(day_end));
        if range_start < range_end {
            let fingerprints = raw_fingerprints(
                database,
                application_id,
                environment_id,
                Some(range_start),
                Some(range_end),
                None,
            )
            .await?;
            if !fingerprints.is_empty() {
                fallback_sets.push(ScopedUserSet {
                    application_id: application_id.unwrap_or(GLOBAL_ENVIRONMENT).to_owned(),
                    day,
                    fingerprints,
                });
            }
        }
    }

    if dirty.len() > MAX_DIRTY_SCOPE_DAYS {
        return Ok(None);
    }
    for (dirty_app, day) in dirty {
        sets.retain(|stored| stored.application_id != dirty_app || stored.day != day);
        let fingerprints = raw_fingerprints(
            database,
            Some(&dirty_app),
            environment_id,
            None,
            None,
            Some(&day),
        )
        .await?;
        if !fingerprints.is_empty() {
            fallback_sets.push(ScopedUserSet {
                application_id: dirty_app,
                day,
                fingerprints,
            });
        }
    }
    sets.extend(fallback_sets);

    let mut by_day = BTreeMap::<String, Vec<Vec<Fingerprint>>>::new();
    for set in sets {
        by_day.entry(set.day).or_default().push(set.fingerprints);
    }
    Ok(Some(
        by_day
            .into_iter()
            .map(|(day, sets)| DailyUserSet {
                day,
                fingerprints: union_sorted_sets(sets),
            })
            .collect(),
    ))
}

async fn raw_fingerprints(
    database: &impl ConnectionTrait,
    application_id: Option<&str>,
    environment_id: Option<&str>,
    since: Option<i64>,
    until: Option<i64>,
    day: Option<&str>,
) -> Result<Vec<Fingerprint>, DbErr> {
    let mut query = Query::select();
    query
        .column(Alias::new("anonymous_id"))
        .from(Alias::new("events"))
        .and_where(Expr::col(Alias::new("anonymous_id")).is_not_null())
        .distinct();
    if let Some(application_id) = application_id {
        query.and_where(Expr::col(Alias::new("application_id")).eq(application_id));
    }
    if let Some(environment_id) = environment_id {
        query.and_where(Expr::col(Alias::new("environment_id")).eq(environment_id));
    }
    if let Some(since) = since {
        query.and_where(Expr::col(Alias::new("timestamp")).gte(since));
    }
    if let Some(until) = until {
        query.and_where(Expr::col(Alias::new("timestamp")).lt(until));
    }
    if let Some(day) = day {
        query.and_where(Expr::col(Alias::new("day")).eq(day));
    }

    let mut fingerprints = Vec::new();
    for row in database.query_all(&query).await? {
        let anonymous_id: String = row.try_get("", "anonymous_id")?;
        fingerprints.push(fingerprint(&anonymous_id));
    }
    fingerprints.sort_unstable();
    fingerprints.dedup();
    Ok(fingerprints)
}

fn partial_boundary_days(
    since: Option<i64>,
    until: Option<i64>,
) -> Result<BTreeSet<String>, DbErr> {
    let mut days = BTreeSet::new();
    if let Some(since) = since {
        let day = day_for_timestamp(since)
            .ok_or_else(|| DbErr::Custom("invalid unique-user range start".into()))?;
        let (day_start, _) = day_bounds(&day)?;
        if since > day_start {
            days.insert(day);
        }
    }
    if let Some(until) = until {
        let probe = until
            .checked_sub(1)
            .ok_or_else(|| DbErr::Custom("invalid unique-user range end".into()))?;
        let day = day_for_timestamp(probe)
            .ok_or_else(|| DbErr::Custom("invalid unique-user range end".into()))?;
        let (_, day_end) = day_bounds(&day)?;
        if until < day_end {
            days.insert(day);
        }
    }
    Ok(days)
}

fn fingerprint(value: &str) -> Fingerprint {
    let digest = Sha256::digest(value.as_bytes());
    let mut fingerprint = [0_u8; FINGERPRINT_BYTES];
    fingerprint.copy_from_slice(&digest[..FINGERPRINT_BYTES]);
    fingerprint
}

fn encode_fingerprints(values: &[Fingerprint]) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(values.len().saturating_mul(FINGERPRINT_BYTES));
    for value in values {
        encoded.extend_from_slice(value);
    }
    encoded
}

fn decode_fingerprints(encoded: &[u8]) -> Result<Vec<Fingerprint>, DbErr> {
    if encoded.len() % FINGERPRINT_BYTES != 0 {
        return Err(DbErr::Custom("invalid daily user-set fingerprint blob".into()));
    }
    encoded
        .chunks_exact(FINGERPRINT_BYTES)
        .map(|chunk| {
            chunk
                .try_into()
                .map_err(|_| DbErr::Custom("invalid daily user-set fingerprint width".into()))
        })
        .collect()
}

fn union_sorted_sets(mut sets: Vec<Vec<Fingerprint>>) -> Vec<Fingerprint> {
    if sets.is_empty() {
        return Vec::new();
    }
    while sets.len() > 1 {
        let mut next = Vec::with_capacity(sets.len().div_ceil(2));
        let mut iter = sets.into_iter();
        while let Some(left) = iter.next() {
            if let Some(right) = iter.next() {
                next.push(merge_sorted_unique(left, right));
            } else {
                next.push(left);
            }
        }
        sets = next;
    }
    sets.pop().unwrap_or_default()
}

fn merge_sorted_unique(left: Vec<Fingerprint>, right: Vec<Fingerprint>) -> Vec<Fingerprint> {
    merge_sorted_unique_count_new(left, right).0
}

fn merge_sorted_unique_count_new(
    left: Vec<Fingerprint>,
    right: Vec<Fingerprint>,
) -> (Vec<Fingerprint>, usize) {
    let mut merged = Vec::with_capacity(left.len().saturating_add(right.len()));
    let mut new_items = 0_usize;
    let mut left_index = 0_usize;
    let mut right_index = 0_usize;
    while left_index < left.len() && right_index < right.len() {
        match left[left_index].cmp(&right[right_index]) {
            std::cmp::Ordering::Less => {
                merged.push(left[left_index]);
                left_index += 1;
            }
            std::cmp::Ordering::Greater => {
                merged.push(right[right_index]);
                right_index += 1;
                new_items = new_items.saturating_add(1);
            }
            std::cmp::Ordering::Equal => {
                merged.push(left[left_index]);
                left_index += 1;
                right_index += 1;
            }
        }
    }
    merged.extend_from_slice(&left[left_index..]);
    new_items = new_items.saturating_add(right.len().saturating_sub(right_index));
    merged.extend_from_slice(&right[right_index..]);
    (merged, new_items)
}

async fn insert_user_set_rows(
    database: &impl ConnectionTrait,
    rows: Vec<Vec<Value>>,
) -> Result<(), DbErr> {
    for chunk in rows.chunks(USER_SET_INSERT_CHUNK) {
        let mut query = Query::insert();
        query
            .into_table(Alias::new("telemetry_daily_user_sets"))
            .columns(
                [
                    "id",
                    "application_id",
                    "environment_id",
                    "day",
                    "chunk_index",
                    "user_count",
                    "fingerprints",
                    "updated_at",
                ]
                .map(Alias::new),
            );
        for row in chunk {
            query
                .values(row.iter().cloned().map(Expr::value))
                .map_err(|error| DbErr::Custom(error.to_string()))?;
        }
        database.execute(&query).await?;
    }
    Ok(())
}

async fn set_system_state(
    database: &impl ConnectionTrait,
    key: &str,
    value: &str,
) -> Result<(), DbErr> {
    let mut query = Query::insert();
    query
        .into_table(Alias::new("system_state"))
        .columns([Alias::new("key"), Alias::new("value")])
        .values(
            [Value::from(key.to_owned()), Value::from(value.to_owned())]
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

fn user_set_id(application_id: &str, environment_id: &str, day: &str, chunk_index: i32) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"sonde:user-rollup:v2\0");
    hasher.update(application_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(environment_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(day.as_bytes());
    hasher.update(b"\0");
    hasher.update(chunk_index.to_le_bytes());
    format!("us_{}", hex::encode(hasher.finalize()))
}

fn day_for_timestamp(timestamp: i64) -> Option<String> {
    chrono::DateTime::from_timestamp_millis(timestamp)
        .map(|value| value.format("%Y-%m-%d").to_string())
}

fn day_start_timestamp(day: &str) -> Option<i64> {
    Some(
        NaiveDate::parse_from_str(day, "%Y-%m-%d")
            .ok()?
            .and_hms_opt(0, 0, 0)?
            .and_utc()
            .timestamp_millis(),
    )
}

fn day_bounds(day: &str) -> Result<(i64, i64), DbErr> {
    let date = NaiveDate::parse_from_str(day, "%Y-%m-%d")
        .map_err(|error| DbErr::Custom(error.to_string()))?;
    let start = date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| DbErr::Custom("invalid user rollup day".into()))?
        .and_utc()
        .timestamp_millis();
    Ok((start, start.saturating_add(86_400_000)))
}

fn positive_u64(value: i64) -> u64 {
    value.max(0) as u64
}

fn saturating_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{
        FINGERPRINTS_PER_CHUNK, decode_fingerprints, encode_fingerprints, fingerprint,
        merge_sorted_unique_count_new, union_sorted_sets,
    };

    #[test]
    fn fingerprint_encoding_and_union_are_stable() {
        let alpha = fingerprint("alpha");
        let beta = fingerprint("beta");
        let gamma = fingerprint("gamma");
        let mut left = vec![alpha, beta];
        let mut right = vec![beta, gamma];
        left.sort_unstable();
        right.sort_unstable();

        let merged = union_sorted_sets(vec![left, right]);
        assert_eq!(merged.len(), 3);
        assert_eq!(decode_fingerprints(&encode_fingerprints(&merged)).unwrap(), merged);
        assert_eq!(fingerprint("alpha"), alpha);
    }

    #[test]
    fn merge_reports_only_previously_unseen_fingerprints() {
        let mut left = vec![fingerprint("a"), fingerprint("b")];
        let mut right = vec![fingerprint("b"), fingerprint("c"), fingerprint("d")];
        left.sort_unstable();
        right.sort_unstable();
        let (merged, new_items) = merge_sorted_unique_count_new(left, right);
        assert_eq!(new_items, 2);
        assert_eq!(merged.len(), 4);
    }

    #[test]
    fn configured_chunk_stays_below_standard_blob_limit() {
        assert!(FINGERPRINTS_PER_CHUNK * 16 < 65_535);
    }
}
