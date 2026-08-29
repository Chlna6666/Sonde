#![allow(clippy::unwrap_used)]

use sea_orm::{
    ConnectionTrait,
    sea_query::{Alias, Expr, ExprTrait, Func, Query},
};
use sonde::{
    database::{
        self, app_repo, application_delete_repo, dimension_rollup_repo, first_seen_repo,
        log_error_rollup_repo, rollup_repo, telemetry_repo, user_rollup_repo,
    },
    domain::telemetry::{Attributes, EventInput, LogInput, LogLevel},
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

fn error_log(timestamp: i64) -> LogInput {
    LogInput {
        level: LogLevel::Error,
        message: "delete-me".into(),
        logger: Some("test".into()),
        trace_id: None,
        span_id: None,
        timestamp: Some(timestamp),
        attributes: Attributes::new(),
    }
}

async fn count_for_app(
    database: &sea_orm::DatabaseConnection,
    table: &str,
    application_id: &str,
) -> i64 {
    let query = Query::select()
        .expr_as(
            Func::count(Expr::col(Alias::new("id"))),
            Alias::new("total"),
        )
        .from(Alias::new(table))
        .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
        .to_owned();
    database
        .query_one(&query)
        .await
        .unwrap()
        .and_then(|row| row.try_get::<i64>("", "total").ok())
        .unwrap_or(0)
}

async fn finish_first_seen_backfill(database: &sea_orm::DatabaseConnection) {
    loop {
        if first_seen_repo::run_backfill_batch(database, 32)
            .await
            .unwrap()
            == 0
        {
            break;
        }
    }
    assert!(first_seen_repo::backfill_complete(database).await.unwrap());
}

async fn process_all_rollups(database: &sea_orm::DatabaseConnection) {
    loop {
        let dirty = rollup_repo::list_dirty_days(database, 64, i64::MAX)
            .await
            .unwrap();
        if dirty.is_empty() {
            break;
        }
        for item in dirty {
            if item.has_source(rollup_repo::DIRTY_SOURCE_EVENT) {
                if !dimension_rollup_repo::recompute_claimed_day_dimensions(database, &item)
                    .await
                    .unwrap()
                {
                    continue;
                }
                if !first_seen_repo::refresh_dirty_day(database, &item)
                    .await
                    .unwrap()
                {
                    continue;
                }
                if !user_rollup_repo::recompute_claimed_day_user_set(database, &item)
                    .await
                    .unwrap()
                {
                    continue;
                }
            }
            if item.has_source(rollup_repo::DIRTY_SOURCE_LOG)
                && !log_error_rollup_repo::recompute_claimed_day(database, &item)
                    .await
                    .unwrap()
            {
                continue;
            }
            let _ = rollup_repo::recompute_claimed_day(database, item)
                .await
                .unwrap();
        }
    }
}

#[tokio::test]
async fn deleting_application_removes_raw_and_derived_state_without_touching_other_app_events() {
    let database = database::connect("sqlite::memory:").await.unwrap();
    database::migrate(&database).await.unwrap();
    let (delete_app, delete_env) =
        app_repo::create_application(&database, "Delete", "delete-app", None)
            .await
            .unwrap();
    let (keep_app, keep_env) = app_repo::create_application(&database, "Keep", "keep-app", None)
        .await
        .unwrap();
    let start = chrono::NaiveDate::from_ymd_opt(2026, 8, 20)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis();

    let delete_scope = telemetry_repo::TelemetryScope {
        application_id: delete_app.clone(),
        environment_id: delete_env,
    };
    telemetry_repo::insert_events(
        &database,
        &delete_scope,
        &[event(start + 1_000, "delete-1", "shared-user")],
    )
    .await
    .unwrap();
    telemetry_repo::insert_logs(&database, &delete_scope, &[error_log(start + 1_500)])
        .await
        .unwrap();
    telemetry_repo::insert_events(
        &database,
        &telemetry_repo::TelemetryScope {
            application_id: keep_app.clone(),
            environment_id: keep_env,
        },
        &[event(start + 2_000, "keep-1", "shared-user")],
    )
    .await
    .unwrap();

    // The production workers run independently. Seed/finish first-seen before consuming the shared
    // dirty markers so refresh_dirty_day has a current epoch and cannot intentionally defer them.
    finish_first_seen_backfill(&database).await;
    process_all_rollups(&database).await;

    assert!(count_for_app(&database, "telemetry_daily_rollups", &delete_app).await > 0);
    assert!(count_for_app(&database, "telemetry_daily_dimensions", &delete_app).await > 0);
    assert!(count_for_app(&database, "telemetry_daily_user_sets", &delete_app).await > 0);
    assert!(count_for_app(&database, "telemetry_daily_log_errors", &delete_app).await > 0);

    application_delete_repo::delete_application_exact(&database, &delete_app)
        .await
        .unwrap();

    for table in [
        "events",
        "metric_points",
        "logs",
        "error_occurrences",
        "error_groups",
        "telemetry_daily_rollups",
        "telemetry_daily_dimensions",
        "telemetry_daily_user_sets",
        "telemetry_daily_log_errors",
        "telemetry_dirty_days",
        "telemetry_first_seen_backfill_days",
        "daily_aggregates",
        "alert_rules",
        "import_runs",
        "api_keys",
        "environments",
        "role_bindings",
    ] {
        assert_eq!(count_for_app(&database, table, &delete_app).await, 0, "{table}");
    }
    assert_eq!(count_for_app(&database, "events", &keep_app).await, 1);

    // Application deletion invalidates the current first-seen epoch in O(1). Old rows may still be
    // physically present but are unreachable; the next rebuild from surviving events makes a fresh
    // epoch authoritative and then garbage-collects old epochs.
    assert!(!first_seen_repo::backfill_complete(&database).await.unwrap());
    finish_first_seen_backfill(&database).await;
    assert_eq!(
        count_for_app(&database, "telemetry_user_first_seen", &delete_app).await,
        0
    );
    assert!(count_for_app(&database, "telemetry_user_first_seen", &keep_app).await > 0);
}
