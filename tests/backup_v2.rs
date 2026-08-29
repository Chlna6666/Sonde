use std::{error::Error, io};

use futures_util::{StreamExt, pin_mut};
use sea_orm::{
    ConnectionTrait,
    sea_query::{Alias, Expr, ExprTrait, Func, Query},
};
use sonde::database::{
    self, app_repo, auth_repo, backup_v2_repo, backup_v2_restore_repo, query::insert,
};
use tokio::io::AsyncWriteExt;

const DERIVED_STATE_KEYS: &[&str] = &[
    "telemetry_dimension_rollup_backfill_v1",
    "telemetry_user_rollup_backfill_v1",
    "telemetry_log_error_rollup_backfill_v2",
    "telemetry_first_seen_backfill_v1",
    "telemetry_first_seen_backfill_cursor_v1",
];

#[tokio::test]
async fn full_backup_v2_round_trip_replaces_state_and_resets_ephemeral_auth(
) -> Result<(), Box<dyn Error>> {
    let source = database::connect("sqlite::memory:").await?;
    database::migrate(&source).await?;
    auth_repo::create_super_admin(
        &source,
        "source@example.test",
        "source-admin",
        "source-password-hash",
        "en",
    )
    .await?;
    let source_user = auth_repo::user_by_email(&source, "source@example.test")
        .await?
        .ok_or_else(|| io::Error::other("source user missing"))?;
    auth_repo::enable_totp(&source, &source_user.id, "SOURCE-TOTP-SEED").await?;

    let (source_app_id, source_env_id) = app_repo::create_application(
        &source,
        "Source App",
        "source-app",
        Some(&source_user.id),
    )
    .await?;
    let now = chrono::Utc::now().timestamp_millis();
    insert(
        &source,
        "events",
        &[
            "id",
            "application_id",
            "environment_id",
            "name",
            "timestamp",
            "day",
            "anonymous_id",
            "session_id",
            "app_version",
            "launcher_version",
            "os",
            "attributes",
            "dedupe_key",
            "received_at",
        ],
        vec![
            uuid::Uuid::now_v7().to_string().into(),
            source_app_id.into(),
            source_env_id.into(),
            "backup.test".into(),
            now.into(),
            chrono::Utc::now().format("%Y-%m-%d").to_string().into(),
            "device-hash".into(),
            Option::<String>::None.into(),
            "1.0.0".into(),
            Option::<String>::None.into(),
            "Linux".into(),
            "{}".into(),
            Option::<String>::None.into(),
            now.into(),
        ],
    )
    .await?;

    let archive = tempfile::NamedTempFile::new()?;
    write_backup(&source, &archive).await?;
    let manifest = backup_v2_repo::validate_backup_file(archive.path()).await?;
    assert_eq!(manifest.format_version, backup_v2_repo::FORMAT_VERSION);
    assert!(!manifest.totp_secrets_included);

    let target = database::connect("sqlite::memory:").await?;
    database::migrate(&target).await?;
    auth_repo::create_super_admin(
        &target,
        "target@example.test",
        "target-admin",
        "target-password-hash",
        "en",
    )
    .await?;
    let target_user = auth_repo::user_by_email(&target, "target@example.test")
        .await?
        .ok_or_else(|| io::Error::other("target bootstrap user missing"))?;
    let (stale_app_id, stale_env_id) = app_repo::create_application(
        &target,
        "Stale Target App",
        "stale-target-app",
        Some(&target_user.id),
    )
    .await?;
    insert(
        &target,
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
            "stale-session-token".into(),
            target_user.id.into(),
            "stale-csrf".into(),
            (now + 60_000).into(),
            now.into(),
            now.into(),
        ],
    )
    .await?;
    seed_stale_derived_state(&target, &stale_app_id, &stale_env_id, now).await?;
    assert_eq!(count_rows(&target, "telemetry_daily_dimensions").await?, 1);
    assert_eq!(count_rows(&target, "telemetry_daily_user_sets").await?, 1);
    assert_eq!(count_rows(&target, "telemetry_daily_log_errors").await?, 1);
    for key in DERIVED_STATE_KEYS {
        assert!(system_state_value(&target, key).await?.is_some());
    }

    let restored =
        backup_v2_restore_repo::restore_full_system_exact(&target, archive.path()).await?;
    assert!(restored > 0);

    let applications = app_repo::list_applications(&target, None, true).await?;
    assert_eq!(applications.len(), 1);
    assert_eq!(applications[0].slug, "source-app");
    assert!(auth_repo::user_by_email(&target, "target@example.test")
        .await?
        .is_none());
    let restored_user = auth_repo::user_by_email(&target, "source@example.test")
        .await?
        .ok_or_else(|| io::Error::other("restored user missing"))?;
    let (totp_enabled, totp_secret) = auth_repo::get_totp_info(&target, &restored_user.id).await?;
    assert!(!totp_enabled);
    assert!(totp_secret.is_none());
    assert_eq!(count_rows(&target, "auth_sessions").await?, 0);
    assert_eq!(count_rows(&target, "events").await?, 1);
    assert_eq!(count_rows(&target, "telemetry_daily_dimensions").await?, 0);
    assert_eq!(count_rows(&target, "telemetry_daily_user_sets").await?, 0);
    assert_eq!(count_rows(&target, "telemetry_daily_log_errors").await?, 0);
    for key in DERIVED_STATE_KEYS {
        assert!(system_state_value(&target, key).await?.is_none(), "{key}");
    }
    Ok(())
}

#[tokio::test]
async fn corrupted_backup_is_rejected_before_target_is_modified() -> Result<(), Box<dyn Error>> {
    let source = database::connect("sqlite::memory:").await?;
    database::migrate(&source).await?;
    auth_repo::create_super_admin(
        &source,
        "source@example.test",
        "source-admin",
        "source-password-hash",
        "en",
    )
    .await?;
    let source_user = auth_repo::user_by_email(&source, "source@example.test")
        .await?
        .ok_or_else(|| io::Error::other("source user missing"))?;
    let _ = app_repo::create_application(
        &source,
        "Source App",
        "source-app",
        Some(&source_user.id),
    )
    .await?;

    let archive = tempfile::NamedTempFile::new()?;
    write_backup(&source, &archive).await?;
    let valid = tokio::fs::read_to_string(archive.path()).await?;
    let tampered = valid.replacen("Source App", "Tampered App", 1);
    if tampered == valid {
        return Err(io::Error::other("test archive did not contain expected source app").into());
    }
    let corrupt = tempfile::NamedTempFile::new()?;
    tokio::fs::write(corrupt.path(), tampered).await?;

    let target = database::connect("sqlite::memory:").await?;
    database::migrate(&target).await?;
    auth_repo::create_super_admin(
        &target,
        "target@example.test",
        "target-admin",
        "target-password-hash",
        "en",
    )
    .await?;
    let target_user = auth_repo::user_by_email(&target, "target@example.test")
        .await?
        .ok_or_else(|| io::Error::other("target user missing"))?;
    let _ = app_repo::create_application(
        &target,
        "Keep Me",
        "keep-me",
        Some(&target_user.id),
    )
    .await?;

    let result = backup_v2_restore_repo::restore_full_system_exact(&target, corrupt.path()).await;
    assert!(result.is_err());

    let applications = app_repo::list_applications(&target, None, true).await?;
    assert_eq!(applications.len(), 1);
    assert_eq!(applications[0].slug, "keep-me");
    assert!(auth_repo::user_by_email(&target, "target@example.test")
        .await?
        .is_some());
    Ok(())
}

async fn seed_stale_derived_state(
    database: &sea_orm::DatabaseConnection,
    application_id: &str,
    environment_id: &str,
    now: i64,
) -> Result<(), Box<dyn Error>> {
    insert(
        database,
        "telemetry_daily_dimensions",
        &[
            "id",
            "application_id",
            "environment_id",
            "day",
            "dimension",
            "dimension_value",
            "count",
            "updated_at",
        ],
        vec![
            "stale-dimension".into(),
            application_id.into(),
            environment_id.into(),
            "2026-08-01".into(),
            "app_version".into(),
            "stale".into(),
            99_i64.into(),
            now.into(),
        ],
    )
    .await?;
    insert(
        database,
        "telemetry_daily_user_sets",
        &[
            "id",
            "application_id",
            "environment_id",
            "day",
            "chunk_index",
            "user_count",
            "fingerprints",
            "updated_at",
        ],
        vec![
            "stale-user-set".into(),
            application_id.into(),
            environment_id.into(),
            "2026-08-01".into(),
            0_i32.into(),
            1_i64.into(),
            vec![0_u8; 16].into(),
            now.into(),
        ],
    )
    .await?;
    insert(
        database,
        "telemetry_daily_log_errors",
        &[
            "id",
            "application_id",
            "environment_id",
            "day",
            "error_logs",
            "updated_at",
        ],
        vec![
            "stale-log-errors".into(),
            application_id.into(),
            environment_id.into(),
            "2026-08-01".into(),
            99_i64.into(),
            now.into(),
        ],
    )
    .await?;
    for key in DERIVED_STATE_KEYS {
        insert(
            database,
            "system_state",
            &["key", "value"],
            vec![(*key).into(), "stale".into()],
        )
        .await?;
    }
    Ok(())
}

async fn system_state_value(
    database: &sea_orm::DatabaseConnection,
    key: &str,
) -> Result<Option<String>, Box<dyn Error>> {
    let query = Query::select()
        .column(Alias::new("value"))
        .from(Alias::new("system_state"))
        .and_where(Expr::col(Alias::new("key")).eq(key))
        .limit(1)
        .to_owned();
    Ok(database
        .query_one(&query)
        .await?
        .and_then(|row| row.try_get::<String>("", "value").ok()))
}

async fn write_backup(
    database: &sea_orm::DatabaseConnection,
    archive: &tempfile::NamedTempFile,
) -> Result<(), Box<dyn Error>> {
    let file = archive.reopen()?;
    let mut file = tokio::fs::File::from_std(file);
    let stream = backup_v2_repo::export_full_system_stream(database.clone());
    pin_mut!(stream);
    while let Some(chunk) = stream.next().await {
        file.write_all(&chunk?).await?;
    }
    file.flush().await?;
    file.sync_all().await?;
    Ok(())
}

async fn count_rows(
    database: &sea_orm::DatabaseConnection,
    table: &str,
) -> Result<u64, Box<dyn Error>> {
    let query = Query::select()
        .expr_as(Func::count(Expr::col(Alias::new("*"))), Alias::new("total"))
        .from(Alias::new(table))
        .to_owned();
    let count = database
        .query_one(&query)
        .await?
        .and_then(|row| row.try_get::<i64>("", "total").ok())
        .unwrap_or(0)
        .max(0) as u64;
    Ok(count)
}
