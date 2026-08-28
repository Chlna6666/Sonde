#![allow(clippy::unwrap_used)]

use sea_orm::{ConnectionTrait, sea_query::{Alias, Expr, Func, Query}};
use sonde::{
    database::{self, first_seen_repo, telemetry_repo},
    domain::telemetry::{Attributes, EventInput},
};

fn event(timestamp: i64, index: usize) -> EventInput {
    EventInput {
        name: "application.start".into(),
        timestamp: Some(timestamp),
        anonymous_id: Some(format!("user-{index}")),
        session_id: Some(format!("session-{index}")),
        app_version: Some("1.0.0".into()),
        launcher_version: Some("1.0.0".into()),
        os: Some("test".into()),
        idempotency_key: Some(format!("event-{index}")),
        attributes: Attributes::new(),
    }
}

async fn pending_count(database: &sea_orm::DatabaseConnection) -> u64 {
    database
        .query_one(
            &Query::select()
                .expr_as(Func::count(Expr::col(Alias::new("id"))), Alias::new("total"))
                .from(Alias::new("telemetry_first_seen_backfill_days"))
                .to_owned(),
        )
        .await
        .unwrap()
        .and_then(|row| row.try_get::<i64>("", "total").ok())
        .unwrap_or(0)
        .max(0) as u64
}

#[tokio::test]
async fn first_seen_seed_is_keyset_paged_instead_of_materializing_all_scope_days() {
    let database = database::connect("sqlite::memory:").await.unwrap();
    database::migrate(&database).await.unwrap();
    let scope = telemetry_repo::TelemetryScope {
        application_id: "app-paging".into(),
        environment_id: "prod".into(),
    };
    let start = chrono::NaiveDate::from_ymd_opt(2025, 1, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis();
    let events = (0..600_usize)
        .map(|index| event(start + index as i64 * 86_400_000 + 1_000, index))
        .collect::<Vec<_>>();
    telemetry_repo::insert_events(&database, &scope, &events)
        .await
        .unwrap();

    assert_eq!(first_seen_repo::run_backfill_batch(&database, 32).await.unwrap(), 512);
    assert_eq!(pending_count(&database).await, 512);
    assert!(!first_seen_repo::backfill_complete(&database).await.unwrap());

    assert_eq!(first_seen_repo::run_backfill_batch(&database, 32).await.unwrap(), 88);
    assert_eq!(pending_count(&database).await, 600);

    // One extra iteration reaches EOF and transitions seeding -> seeded. It deliberately reports a
    // sentinel work item so drain-until-zero callers continue into the processing phase.
    assert_eq!(first_seen_repo::run_backfill_batch(&database, 32).await.unwrap(), 1);
    assert!(!first_seen_repo::backfill_complete(&database).await.unwrap());
}
