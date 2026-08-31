#![allow(clippy::expect_used)]

use sha2::Digest;
use sonde::{
    auth,
    database::{self, auth as auth_store},
};

#[tokio::test]
async fn sqlite_migration_and_super_admin_creation_are_usable() {
    let database = database::connect("sqlite::memory:")
        .await
        .expect("in-memory SQLite should connect");
    database::migrate(&database)
        .await
        .expect("schema migration should complete");
    let password_hash = auth::hash_password("Orbit-lantern-27-river", b"test-pepper")
        .expect("test password should hash");
    auth_store::create_super_admin(
        &database,
        "owner@example.com",
        "Super Admin",
        &password_hash,
        "en",
    )
    .await
    .expect("owner should be created");
    let owner = auth_store::user_by_email(&database, "owner@example.com")
        .await
        .expect("query should succeed");
    assert!(owner.is_some(), "owner must be readable after setup");

    let owner_by_username = auth_store::user_by_identifier(&database, "Super Admin")
        .await
        .expect("query by username should succeed");
    assert!(
        owner_by_username.is_some(),
        "owner must be readable by username"
    );

    auth_store::create_super_admin(
        &database,
        "owner@example.com",
        "Updated Admin",
        &password_hash,
        "zh-CN",
    )
    .await
    .expect("repeated super admin creation should be idempotent and succeed");
    let updated_owner = auth_store::user_by_email(&database, "owner@example.com")
        .await
        .expect("query should succeed");
    assert_eq!(
        updated_owner.map(|u| u.username),
        Some("Updated Admin".to_string())
    );

    let updated_by_username = auth_store::user_by_identifier(&database, "Updated Admin")
        .await
        .expect("query by updated username should succeed");
    assert!(
        updated_by_username.is_some(),
        "updated owner must be readable by username"
    );
}

#[tokio::test]
async fn application_management_and_api_keys_and_stats() {
    let database = database::connect("sqlite::memory:")
        .await
        .expect("in-memory SQLite should connect");
    database::migrate(&database)
        .await
        .expect("schema migration should complete");

    let (app_id, env_id) =
        database::applications::create_application(&database, "Demo App", "demo-app", None)
            .await
            .expect("app creation should succeed");

    let raw_key = "sonde_1234567890abcdef1234567890abcdef";
    let key_hash = hex::encode(sha2::Sha256::digest(raw_key.as_bytes()));
    let key_id = database::applications::create_api_key(
        &database,
        &app_id,
        &env_id,
        "Production Key",
        &key_hash,
        "sonde_123456",
        &["ingest".to_string()],
    )
    .await
    .expect("create api key should succeed");

    let keys = database::applications::list_api_keys(&database, &app_id)
        .await
        .expect("listing keys should succeed");
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].name, "Production Key");
    assert!(keys[0].is_active);

    database::applications::update_application(
        &database,
        &app_id,
        database::applications::UpdateApplicationParams {
            name: "Demo App Renamed",
            slug: "demo-app-v2",
            retention_days: 180,
            is_public: Some(true),
            description: Some(Some("A demo telemetry app".into())),
            github_url: Some(Some("https://github.com/example/demo".into())),
            website_url: Some(Some("https://example.com".into())),
            custom_header: Some(Some("v2.0.0".into())),
        },
    )
    .await
    .expect("app update should succeed");

    let apps = database::applications::list_applications(&database, None, true)
        .await
        .expect("list apps should succeed");
    assert_eq!(apps[0].name, "Demo App Renamed");
    assert_eq!(apps[0].retention_days, 180);
    assert!(apps[0].is_public);
    assert_eq!(apps[0].description.as_deref(), Some("A demo telemetry app"));

    let revoked = database::applications::revoke_api_key(&database, &app_id, &key_id)
        .await
        .expect("revoke key should succeed");
    assert!(revoked);

    let keys_after_revoke = database::applications::list_api_keys(&database, &app_id)
        .await
        .expect("list keys should succeed");
    assert!(!keys_after_revoke[0].is_active);

    let key2_id = database::applications::create_api_key(
        &database,
        &app_id,
        &env_id,
        "Temporary Key",
        "hash_temp",
        "sonde_temp",
        &["ingest".to_string()],
    )
    .await
    .expect("create key2 should succeed");

    let deleted = database::applications::delete_api_key(&database, &app_id, &key2_id)
        .await
        .expect("delete key should succeed");
    assert!(deleted);

    let keys_after_del = database::applications::list_api_keys(&database, &app_id)
        .await
        .expect("list keys should succeed");
    assert_eq!(keys_after_del.len(), 1);
    assert_eq!(keys_after_del[0].id, key_id);

    let deleted_revoked_count =
        database::applications::delete_revoked_api_keys(&database, &app_id)
            .await
            .expect("delete revoked keys should succeed");
    assert_eq!(deleted_revoked_count, 1);

    let keys_after_clear_revoked = database::applications::list_api_keys(&database, &app_id)
        .await
        .expect("list apps should succeed");
    assert_eq!(keys_after_clear_revoked.len(), 0);

    let _ = database::applications::create_api_key(
        &database,
        &app_id,
        &env_id,
        "Production Key",
        &key_hash,
        "sonde_123456",
        &["ingest".to_string()],
    )
    .await
    .expect("create api key should succeed");

    let stats = database::stats::application_stats(&database, &app_id, None, Some(30))
        .await
        .expect("application stats query should succeed");
    assert_eq!(stats.overview.total_events, 0);

    let export_payload = database::application_backup::export_single_application(&database, &app_id)
        .await
        .expect("export should succeed")
        .expect("application export payload should exist");
    assert_eq!(export_payload.application.name, "Demo App Renamed");
    assert_eq!(export_payload.api_keys.len(), 1);
    assert_eq!(
        export_payload.format_version,
        database::application_backup::FORMAT_VERSION
    );

    let imported_app_id = database::application_backup::import_single_application(
        &database,
        None,
        export_payload,
    )
    .await
    .expect("import single application should succeed");
    let apps_after_import = database::applications::list_applications(&database, None, true)
        .await
        .expect("list apps should succeed");
    assert_eq!(apps_after_import.len(), 2);

    database::applications::delete_application(&database, &app_id)
        .await
        .expect("delete application should succeed");
    database::applications::delete_application(&database, &imported_app_id)
        .await
        .expect("delete application should succeed");

    let apps_after_delete = database::applications::list_applications(&database, None, true)
        .await
        .expect("list apps should succeed");
    assert_eq!(apps_after_delete.len(), 0);
}
