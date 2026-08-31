#![allow(clippy::unwrap_used)]

use sea_orm::{
    ConnectionTrait,
    sea_query::{Alias, Expr, ExprTrait, Func, Query},
};
use sonde::{
    database::{self, rollups, telemetry, user_rollup},
    domain::telemetry::{Attributes, EventInput},
};

fn event(timestamp: i64, key: &str, user: &str) -> EventInput {
    EventInput {
        name: "application.start".into(),
        timestamp: Some(timestamp),
        anonymous_id: Some(user.into()),
        session_id: Some(format!("session-{key}")),
        app_version: Some("1.0.0".into()),
        launcher_version: Some("1.0.0".into()),
        os: Some("test".into()),
        idempotency_key: Some(key.into()),
        attributes: Attributes::new(),
    }
}

async fn recompute_scope_day(
    database: &sea_orm::DatabaseConnection,
    application_id: &str,
    environment_id: &str,
) {
    let dirty = rollups::list_dirty_days(database, 32, i64::MAX)
        .await
        .unwrap()
        .into_iter()
        .find(|item| {
            item.application_id == application_id && item.environment_id == environment_id
        })
        .unwrap();
    assert!(
        user_rollup::recompute_claimed_day_user_set(database, &dirty)
            .await
            .unwrap()
    );
    assert!(rollups::recompute_claimed_day(database, dirty)
        .await
        .unwrap());
}

#[tokio::test]
async fn user_rollup_merges_clean_days_dirty_days_and_partial_boundaries() {
    let database = database::connect("sqlite::memory:").await.unwrap();
    database::migrate(&database).await.unwrap();
    let scope = telemetry::TelemetryScope {
        application_id: "app-a".into(),
        environment_id: "prod".into(),
    };
    let start = chrono::NaiveDate::from_ymd_opt(2026, 8, 20)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis();

    telemetry::insert_events(
        &database,
        &scope,
        &[
            event(start + 3_600_000, "evt-1", "user-a"),
            event(start + 2 * 3_600_000, "evt-2", "user-a"),
            event(start + 13 * 3_600_000, "evt-3", "user-b"),
        ],
    )
    .await
    .unwrap();

    assert_eq!(
        user_rollup::seed_historical_user_dirty_days_once(&database)
            .await
            .unwrap(),
        1
    );
    recompute_scope_day(&database, "app-a", "prod").await;

    assert_eq!(
        user_rollup::unique_users_hybrid(
            &database,
            Some("app-a"),
            Some("prod"),
            Some(start),
            Some(start + 86_400_000),
        )
        .await
        .unwrap(),
        2
    );

    telemetry::insert_events(
        &database,
        &scope,
        &[event(start + 15 * 3_600_000, "evt-4", "user-c")],
    )
    .await
    .unwrap();

    // The stored clean set is now stale, but the dirty marker forces an authoritative raw read.
    assert_eq!(
        user_rollup::unique_users_hybrid(
            &database,
            Some("app-a"),
            Some("prod"),
            Some(start),
            Some(start + 86_400_000),
        )
        .await
        .unwrap(),
        3
    );

    // The first boundary day is only partially included; user-a occurred before noon and must not
    // leak in from the full-day cached set.
    assert_eq!(
        user_rollup::unique_users_hybrid(
            &database,
            Some("app-a"),
            Some("prod"),
            Some(start + 12 * 3_600_000),
            Some(start + 86_400_000),
        )
        .await
        .unwrap(),
        2
    );

    let next_day = start + 86_400_000;
    telemetry::insert_events(
        &database,
        &scope,
        &[
            event(next_day + 3_600_000, "evt-5", "user-b"),
            event(next_day + 2 * 3_600_000, "evt-6", "user-d"),
        ],
    )
    .await
    .unwrap();

    // Cross-day union must deduplicate user-b rather than summing daily unique counts.
    assert_eq!(
        user_rollup::unique_users_hybrid(
            &database,
            Some("app-a"),
            Some("prod"),
            Some(start),
            Some(next_day + 86_400_000),
        )
        .await
        .unwrap(),
        4
    );
}

#[tokio::test]
async fn user_rollup_chunks_large_daily_unique_sets_without_losing_users() {
    let database = database::connect("sqlite::memory:").await.unwrap();
    database::migrate(&database).await.unwrap();
    let scope = telemetry::TelemetryScope {
        application_id: "app-chunk".into(),
        environment_id: "prod".into(),
    };
    let start = chrono::NaiveDate::from_ymd_opt(2026, 8, 21)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis();

    let events = (0..2_050)
        .map(|index| {
            event(
                start + index as i64,
                &format!("evt-{index}"),
                &format!("user-{index}"),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        telemetry::insert_events(&database, &scope, &events)
            .await
            .unwrap(),
        events.len()
    );
    assert_eq!(
        user_rollup::seed_historical_user_dirty_days_once(&database)
            .await
            .unwrap(),
        1
    );
    recompute_scope_day(&database, "app-chunk", "prod").await;

    let chunk_count_query = Query::select()
        .expr_as(
            Func::count(Expr::col(Alias::new("id"))),
            Alias::new("total"),
        )
        .from(Alias::new("telemetry_daily_user_sets"))
        .and_where(Expr::col(Alias::new("application_id")).eq("app-chunk"))
        .and_where(Expr::col(Alias::new("environment_id")).eq("prod"))
        .to_owned();
    let chunk_count = database
        .query_one(&chunk_count_query)
        .await
        .unwrap()
        .and_then(|row| row.try_get::<i64>("", "total").ok())
        .unwrap_or(0);
    assert_eq!(chunk_count, 2);

    assert_eq!(
        user_rollup::unique_users_hybrid(
            &database,
            Some("app-chunk"),
            Some("prod"),
            Some(start),
            Some(start + 86_400_000),
        )
        .await
        .unwrap(),
        2_050
    );
}

#[tokio::test]
async fn user_growth_projection_deduplicates_across_days_and_months() {
    let database = database::connect("sqlite::memory:").await.unwrap();
    database::migrate(&database).await.unwrap();
    let scope = telemetry::TelemetryScope {
        application_id: "app-growth".into(),
        environment_id: "prod".into(),
    };
    let august_31 = chrono::NaiveDate::from_ymd_opt(2026, 8, 31)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis();
    let september_1 = august_31 + 86_400_000;
    let september_2 = september_1 + 86_400_000;

    telemetry::insert_events(
        &database,
        &scope,
        &[
            event(august_31 + 1_000, "growth-1", "user-a"),
            event(august_31 + 2_000, "growth-2", "user-b"),
            event(september_1 + 1_000, "growth-3", "user-b"),
            event(september_1 + 2_000, "growth-4", "user-c"),
            event(september_2 + 1_000, "growth-5", "user-d"),
        ],
    )
    .await
    .unwrap();
    assert_eq!(
        user_rollup::seed_historical_user_dirty_days_once(&database)
            .await
            .unwrap(),
        3
    );
    for _ in 0..3 {
        recompute_scope_day(&database, "app-growth", "prod").await;
    }

    let daily = user_rollup::user_growth_hybrid(
        &database,
        Some("app-growth"),
        Some("prod"),
        Some(august_31),
        false,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(daily.len(), 3);
    assert_eq!((daily[0].new_users, daily[0].cumulative_users, daily[0].active_users), (2, 2, 2));
    assert_eq!((daily[1].new_users, daily[1].cumulative_users, daily[1].active_users), (1, 3, 2));
    assert_eq!((daily[2].new_users, daily[2].cumulative_users, daily[2].active_users), (1, 4, 1));

    let monthly = user_rollup::user_growth_hybrid(
        &database,
        Some("app-growth"),
        Some("prod"),
        Some(august_31),
        true,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(monthly.len(), 2);
    assert_eq!((monthly[0].new_users, monthly[0].cumulative_users, monthly[0].active_users), (2, 2, 2));
    assert_eq!((monthly[1].new_users, monthly[1].cumulative_users, monthly[1].active_users), (2, 4, 3));
}
