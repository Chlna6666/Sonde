#![allow(clippy::unwrap_used)]

use std::{sync::Arc, time::Duration};

use sonde::{
    config::{InstallationConfig, MasterKey},
    database::{self, applications as db_apps},
    domain::{
        permission::PermissionGrant,
        telemetry::{Attributes, EventInput},
    },
    security::AuthSecurity,
    services::{
        access, applications as app_service,
        authentication::AuthenticatedUser,
        explorer as explorer_service,
        telemetry::{self, IngestScope},
    },
    state::InstalledState,
};

#[tokio::test]
async fn test_audit_logging_comprehensive() -> Result<(), Box<dyn std::error::Error>> {
    let database = database::connect("sqlite::memory:").await?;
    database::migrate(&database).await?;

    let config = InstallationConfig {
        database_url: "sqlite::memory:".into(),
        locale: "zh-CN".into(),
        timezone: "Asia/Shanghai".into(),
        secure_cookie: false,
    };
    let pepper = b"test-secret-pepper-32-bytes-long!";
    let master_key = MasterKey::from_bytes([9_u8; 32]);
    let state = InstalledState::new(
        database.clone(),
        config,
        Arc::new(AuthSecurity::new(pepper)?),
        &master_key,
    )?;

    let super_admin = AuthenticatedUser {
        id: "super-admin-id".into(),
        username: "admin".into(),
        email: "admin@example.com".into(),
        locale: "zh-CN".into(),
        roles: vec!["SuperAdmin".into()],
        grants: vec![PermissionGrant {
            application_id: None,
            permissions: vec!["*".into()],
        }],
        totp_enabled: false,
    };

    // 1. Create an application and verify audit log with metadata
    let (app_id, _env_id) =
        app_service::create(&state, &super_admin, "Audit Test App", "audit-test-app").await?;

    let logs =
        db_apps::list_audit_logs(&database, 1, 10, Some("application.created"), None).await?;
    assert_eq!(logs.items.len(), 1);
    let app_create_log = &logs.items[0];
    assert_eq!(app_create_log.action, "application.created");
    assert_eq!(app_create_log.resource_type, "application");
    assert_eq!(app_create_log.resource_id.as_deref(), Some(app_id.as_str()));
    assert_eq!(app_create_log.metadata["name"], "Audit Test App");
    assert_eq!(app_create_log.metadata["slug"], "audit-test-app");

    // 2. Ingest events and test telemetry delete & reset audit logs
    let env_id = "test-env";
    let scope = IngestScope::for_test(&app_id, env_id, "device-audit");
    let event = EventInput {
        name: "audit_ev".into(),
        timestamp: None,
        anonymous_id: None,
        session_id: None,
        app_version: None,
        os: None,
        system_language: None,
        architecture: None,
        attributes: Attributes::default(),
        idempotency_key: None,
    };
    telemetry::events(&state, &scope, vec![event]).await?;
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Delete telemetry records
    let records = explorer_service::events(
        &state,
        &super_admin,
        &explorer_service::ExplorerFilter {
            application_id: app_id.clone(),
            environment_id: Some(env_id.into()),
            from: None,
            to: None,
            name: None,
            level: None,
            text: None,
            page: 1,
            page_size: 10,
        },
    )
    .await?;
    assert_eq!(records.items.len(), 1);
    let event_id = &records.items[0].id;

    let deleted_count = explorer_service::delete_records(
        &state,
        &super_admin,
        "events",
        &app_id,
        Some(env_id),
        std::slice::from_ref(event_id),
    )
    .await?;
    assert_eq!(deleted_count, 1);

    let logs =
        db_apps::list_audit_logs(&database, 1, 10, Some("telemetry.records_deleted"), None).await?;
    assert_eq!(logs.items.len(), 1);
    let telem_del_log = &logs.items[0];
    assert_eq!(telem_del_log.action, "telemetry.records_deleted");
    assert_eq!(telem_del_log.metadata["kind"], "events");
    assert_eq!(telem_del_log.metadata["count"], 1);
    assert_eq!(telem_del_log.metadata["applicationId"], app_id.as_str());
    assert_eq!(telem_del_log.metadata["applicationName"], "Audit Test App");

    // Reset telemetry records
    let reset_count =
        explorer_service::reset_records(&state, &super_admin, "events", &app_id, Some(env_id))
            .await?;
    assert_eq!(reset_count, 0); // Already deleted above

    let logs =
        db_apps::list_audit_logs(&database, 1, 10, Some("telemetry.records_reset"), None).await?;
    assert_eq!(logs.items.len(), 1);
    let telem_reset_log = &logs.items[0];
    assert_eq!(telem_reset_log.action, "telemetry.records_reset");
    assert_eq!(telem_reset_log.metadata["kind"], "events");
    assert_eq!(
        telem_reset_log.metadata["applicationName"],
        "Audit Test App"
    );

    // 3. User operations: create user, reset password, delete user
    let new_user_id = access::create_user(
        &state,
        &super_admin,
        access::CreateUserInput {
            email: "bob@example.com",
            username: "bob",
            password: "sonde-user-bob-pass-1",
            locale: "en",
            role: "Admin",
            pepper,
        },
    )
    .await?;

    let logs = db_apps::list_audit_logs(&database, 1, 10, Some("user.created"), None).await?;
    assert_eq!(logs.items.len(), 1);
    let user_created_log = &logs.items[0];
    assert_eq!(user_created_log.metadata["username"], "bob");
    assert_eq!(user_created_log.metadata["email"], "bob@example.com");
    assert_eq!(user_created_log.metadata["role"], "Admin");

    // Password reset
    access::reset_password(
        &state,
        &super_admin,
        &new_user_id,
        "sonde-user-bob-pass-2",
        pepper,
    )
    .await?;

    let logs =
        db_apps::list_audit_logs(&database, 1, 10, Some("user.password_reset"), None).await?;
    assert_eq!(logs.items.len(), 1);
    let pwd_reset_log = &logs.items[0];
    assert_eq!(pwd_reset_log.metadata["username"], "bob");

    // Delete user
    access::delete_user(&state, &super_admin, &new_user_id).await?;

    let logs = db_apps::list_audit_logs(&database, 1, 10, Some("user.deleted"), None).await?;
    assert_eq!(logs.items.len(), 1);
    let user_del_log = &logs.items[0];
    assert_eq!(user_del_log.metadata["username"], "bob");
    assert_eq!(user_del_log.metadata["email"], "bob@example.com");

    // 4. Delete application and verify that name & slug are preserved in metadata
    app_service::delete(&state, &super_admin, &app_id).await?;

    let logs =
        db_apps::list_audit_logs(&database, 1, 10, Some("application.deleted"), None).await?;
    assert_eq!(logs.items.len(), 1);
    let app_del_log = &logs.items[0];
    assert_eq!(app_del_log.metadata["name"], "Audit Test App");
    assert_eq!(app_del_log.metadata["slug"], "audit-test-app");

    // 5. Query filtering by resource_type
    let telem_logs = db_apps::list_audit_logs(&database, 1, 20, None, Some("telemetry")).await?;
    assert_eq!(telem_logs.items.len(), 2);

    let app_logs = db_apps::list_audit_logs(&database, 1, 20, None, Some("application")).await?;
    assert_eq!(app_logs.items.len(), 2); // created & deleted

    let user_logs = db_apps::list_audit_logs(&database, 1, 20, None, Some("user")).await?;
    assert_eq!(user_logs.items.len(), 3); // created, password_reset, deleted

    Ok(())
}
