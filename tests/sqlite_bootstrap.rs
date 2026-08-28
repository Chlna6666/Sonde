#![allow(clippy::expect_used)]

use sha2::Digest;
use sonde::{
    auth,
    database::{self, auth_repo},
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
    auth_repo::create_super_admin(
        &database,
        "owner@example.com",
        "Super Admin",
        &password_hash,
        "en",
    )
    .await
    .expect("owner should be created");
    let owner = auth_repo::user_by_email(&database, "owner@example.com")
        .await
        .expect("query should succeed");
    assert!(owner.is_some(), "owner must be readable after setup");

    // Test lookup by username
    let owner_by_username = auth_repo::user_by_identifier(&database, "Super Admin")
        .await
        .expect("query by username should succeed");
    assert!(
        owner_by_username.is_some(),
        "owner must be readable by username"
    );

    // Test repeated/idempotent execution
    auth_repo::create_super_admin(
        &database,
        "owner@example.com",
        "Updated Admin",
        &password_hash,
        "zh-CN",
    )
    .await
    .expect("repeated super admin creation should be idempotent and succeed");
    let updated_owner = auth_repo::user_by_email(&database, "owner@example.com")
        .await
        .expect("query should succeed");
    assert_eq!(
        updated_owner.map(|u| u.username),
        Some("Updated Admin".to_string())
    );

    let updated_by_username = auth_repo::user_by_identifier(&database, "Updated Admin")
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

    // 1. Create application
    let (app_id, env_id) =
        database::app_repo::create_application(&database, "Demo App", "demo-app", None)
            .await
            .expect("app creation should succeed");

    // 2. Create API key
    let raw_key = "sonde_1234567890abcdef1234567890abcdef";
    let key_hash = hex::encode(sha2::Sha256::digest(raw_key.as_bytes()));
    let key_id = database::app_repo::create_api_key(
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

    // 3. List keys
    let keys = database::app_repo::list_api_keys(&database, &app_id)
        .await
        .expect("listing keys should succeed");
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].name, "Production Key");
    assert!(keys[0].is_active);

    // 4. Update application
    database::app_repo::update_application(
        &database,
        &app_id,
        database::app_repo::UpdateApplicationParams {
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

    let apps = database::app_repo::list_applications(&database, None, true)
        .await
        .expect("list apps should succeed");
    assert_eq!(apps[0].name, "Demo App Renamed");
    assert_eq!(apps[0].retention_days, 180);
    assert!(apps[0].is_public);
    assert_eq!(apps[0].description.as_deref(), Some("A demo telemetry app"));

    // 5. Revoke key
    let revoked = database::app_repo::revoke_api_key(&database, &app_id, &key_id)
        .await
        .expect("revoke key should succeed");
    assert!(revoked);

    let keys_after_revoke = database::app_repo::list_api_keys(&database, &app_id)
        .await
        .expect("list keys should succeed");
    assert!(!keys_after_revoke[0].is_active);

    // 5.1 Test create another key and delete_api_key
    let key2_id = database::app_repo::create_api_key(
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

    let deleted = database::app_repo::delete_api_key(&database, &app_id, &key2_id)
        .await
        .expect("delete key should succeed");
    assert!(deleted);

    let keys_after_del = database::app_repo::list_api_keys(&database, &app_id)
        .await
        .expect("list keys should succeed");
    assert_eq!(keys_after_del.len(), 1);
    assert_eq!(keys_after_del[0].id, key_id);

    // 5.2 Test delete_revoked_api_keys
    let deleted_revoked_count = database::app_repo::delete_revoked_api_keys(&database, &app_id)
        .await
        .expect("delete revoked keys should succeed");
    assert_eq!(deleted_revoked_count, 1);

    let keys_after_clear_revoked = database::app_repo::list_api_keys(&database, &app_id)
        .await
        .expect("list keys should succeed");
    assert_eq!(keys_after_clear_revoked.len(), 0);

    // Re-create key for backup test
    let _ = database::app_repo::create_api_key(
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

    // 6. Test application stats query
    let stats = database::stats_repo::application_stats(&database, &app_id, None, Some(30))
        .await
        .expect("application stats query should succeed");
    assert_eq!(stats.overview.total_events, 0);

    // 7. Test Export Single Application
    let export_payload = database::backup_repo::export_single_application(&database, &app_id)
        .await
        .expect("export should succeed")
        .expect("application export payload should exist");
    assert_eq!(export_payload.application.name, "Demo App Renamed");
    assert_eq!(export_payload.api_keys.len(), 1);

    // 8. Test Import Single Application
    let imported_app_id =
        database::backup_repo::import_single_application(&database, None, export_payload)
            .await
            .expect("import single application should succeed");
    let apps_after_import = database::app_repo::list_applications(&database, None, true)
        .await
        .expect("list apps should succeed");
    assert_eq!(apps_after_import.len(), 2);

    // 9. Test Full System Backup
    let backup_data = database::backup_repo::export_full_system(&database)
        .await
        .expect("export full system should succeed");
    assert_eq!(backup_data.applications.len(), 2);

    // 10. Delete application
    database::app_repo::delete_application(&database, &app_id)
        .await
        .expect("delete application should succeed");
    database::app_repo::delete_application(&database, &imported_app_id)
        .await
        .expect("delete application should succeed");

    let apps_after_delete = database::app_repo::list_applications(&database, None, true)
        .await
        .expect("list apps should succeed");
    assert_eq!(apps_after_delete.len(), 0);
}
