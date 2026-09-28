#![allow(clippy::unwrap_used)]

use sea_orm::{
    ConnectionTrait,
    sea_query::{Alias, Expr, ExprTrait, Query, Value},
};
use sonde::{
    database::{self, query::insert},
    services::retention,
};

#[tokio::test]
async fn audit_log_retention_prunes_expired_records() {
    let database = database::connect("sqlite::memory:").await.unwrap();
    database::migrate(&database).await.unwrap();

    let now = chrono::Utc::now().timestamp_millis();
    let old_timestamp = now - 200 * 86_400_000;
    let recent_timestamp = now - 5 * 86_400_000;

    for (id, created_at) in [
        ("audit-old-1", old_timestamp),
        ("audit-recent-1", recent_timestamp),
    ] {
        insert(
            &database,
            "audit_log",
            &[
                "id",
                "actor_user_id",
                "action",
                "resource_type",
                "resource_id",
                "metadata",
                "created_at",
            ],
            vec![
                id.into(),
                Value::String(Some("user-1".into())),
                "test_action".into(),
                "test_resource".into(),
                Value::String(None),
                "{}".into(),
                created_at.into(),
            ],
        )
        .await
        .unwrap();
    }

    let report = retention::run_retention_sweep(&database, 180)
        .await
        .unwrap();
    assert_eq!(report.audit_logs_deleted, 1);

    let count_query = Query::select()
        .expr(Expr::col(Alias::new("id")).count())
        .from(Alias::new("audit_log"))
        .and_where(Expr::col(Alias::new("id")).eq("audit-old-1"))
        .to_owned();
    let old_count: i64 = database
        .query_one(&count_query)
        .await
        .unwrap()
        .unwrap()
        .try_get_by_index(0)
        .unwrap();
    assert_eq!(old_count, 0);

    let count_recent = Query::select()
        .expr(Expr::col(Alias::new("id")).count())
        .from(Alias::new("audit_log"))
        .and_where(Expr::col(Alias::new("id")).eq("audit-recent-1"))
        .to_owned();
    let recent_count: i64 = database
        .query_one(&count_recent)
        .await
        .unwrap()
        .unwrap()
        .try_get_by_index(0)
        .unwrap();
    assert_eq!(recent_count, 1);
}
