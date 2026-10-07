#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use sonde::{
    config::{InstallationConfig, MasterKey},
    database,
    domain::{
        permission::PermissionGrant,
        telemetry::{Attributes, EventInput, LogInput, LogLevel, MetricInput, MetricType},
    },
    security::AuthSecurity,
    services::{
        authentication::AuthenticatedUser,
        explorer::{self as explorer_service, ExplorerFilter as ServiceFilter},
        telemetry::{self, IngestScope},
    },
    state::InstalledState,
};

#[tokio::test]
async fn test_explorer_batch_delete_and_reset() -> Result<(), Box<dyn std::error::Error>> {
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

    let app_id = "test-explorer-app";
    let env_id = "test-explorer-env";
    let scope = IngestScope::for_test(app_id, env_id, "device-1");

    // 1. Ingest events (timestamp is server-owned, must be None)
    let events = vec![
        EventInput {
            name: "event_1".into(),
            timestamp: None,
            anonymous_id: None,
            session_id: None,
            app_version: None,
            os: None,
            system_language: None,
            architecture: None,
            attributes: Attributes::default(),
            idempotency_key: None,
        },
        EventInput {
            name: "event_2".into(),
            timestamp: None,
            anonymous_id: None,
            session_id: None,
            app_version: None,
            os: None,
            system_language: None,
            architecture: None,
            attributes: Attributes::default(),
            idempotency_key: None,
        },
        EventInput {
            name: "event_3".into(),
            timestamp: None,
            anonymous_id: None,
            session_id: None,
            app_version: None,
            os: None,
            system_language: None,
            architecture: None,
            attributes: Attributes::default(),
            idempotency_key: None,
        },
    ];
    let ev_receipt = telemetry::events(&state, &scope, events).await?;
    assert_eq!(ev_receipt.accepted, 3);
    assert!(ev_receipt.rejected.is_empty());

    // 2. Ingest metrics (timestamp is server-owned, must be None)
    let metrics = vec![
        MetricInput {
            name: "metric_1".into(),
            metric_type: MetricType::Counter,
            value: Some(10.0),
            histogram: None,
            unit: Some("ms".into()),
            timestamp: None,
            attributes: Attributes::default(),
        },
        MetricInput {
            name: "metric_2".into(),
            metric_type: MetricType::Gauge,
            value: Some(20.0),
            histogram: None,
            unit: Some("ms".into()),
            timestamp: None,
            attributes: Attributes::default(),
        },
    ];
    let met_receipt = telemetry::metrics(&state, &scope, metrics).await?;
    assert_eq!(met_receipt.accepted, 2);
    assert!(met_receipt.rejected.is_empty());

    // 3. Ingest logs (timestamp is server-owned, must be None)
    let logs = vec![
        LogInput {
            level: LogLevel::Info,
            message: "log 1".into(),
            logger: None,
            trace_id: None,
            span_id: None,
            timestamp: None,
            attributes: Attributes::default(),
        },
        LogInput {
            level: LogLevel::Error,
            message: "log 2".into(),
            logger: None,
            trace_id: None,
            span_id: None,
            timestamp: None,
            attributes: Attributes::default(),
        },
    ];
    let log_receipt = telemetry::logs(&state, &scope, logs).await?;
    assert_eq!(log_receipt.accepted, 2);
    assert!(log_receipt.rejected.is_empty());

    // Wait for async queue to persist
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Users
    let admin_user = AuthenticatedUser {
        id: "user-admin".into(),
        email: "admin@test.com".into(),
        username: "admin".into(),
        locale: "zh-CN".into(),
        roles: vec!["Admin".into()],
        grants: vec![PermissionGrant {
            application_id: None,
            permissions: vec!["*".into()],
        }],
        totp_enabled: false,
    };

    let viewer_user = AuthenticatedUser {
        id: "user-viewer".into(),
        email: "viewer@test.com".into(),
        username: "viewer".into(),
        locale: "zh-CN".into(),
        roles: vec!["Viewer".into()],
        grants: vec![PermissionGrant {
            application_id: Some(app_id.into()),
            permissions: vec!["telemetry.read".into()],
        }],
        totp_enabled: false,
    };

    let query_filter = ServiceFilter {
        application_id: app_id.into(),
        environment_id: Some(env_id.into()),
        from: None,
        to: None,
        name: None,
        level: None,
        text: None,
        page: 1,
        page_size: 50,
    };

    // Query events initially
    let initial_events = explorer_service::events(&state, &admin_user, &query_filter).await?;
    assert_eq!(initial_events.items.len(), 3);

    // Test permission: Viewer cannot delete
    let event_id_to_delete = initial_events.items[0].id.clone();
    let delete_result = explorer_service::delete_records(
        &state,
        &viewer_user,
        "events",
        app_id,
        Some(env_id),
        std::slice::from_ref(&event_id_to_delete),
    )
    .await;
    assert!(
        delete_result.is_err(),
        "viewer should not have permission to delete records"
    );

    // Test Admin batch delete 1 event
    let deleted_count = explorer_service::delete_records(
        &state,
        &admin_user,
        "events",
        app_id,
        Some(env_id),
        &[event_id_to_delete],
    )
    .await?;
    assert_eq!(deleted_count, 1);

    // Verify 2 events remain
    let remaining_events = explorer_service::events(&state, &admin_user, &query_filter).await?;
    assert_eq!(remaining_events.items.len(), 2);

    // Test Reset specific kind: reset events only
    let reset_events_count =
        explorer_service::reset_records(&state, &admin_user, "events", app_id, Some(env_id))
            .await?;
    assert_eq!(reset_events_count, 2);

    let empty_events = explorer_service::events(&state, &admin_user, &query_filter).await?;
    assert_eq!(empty_events.items.len(), 0);

    // Verify metrics and logs still exist
    let remaining_metrics = explorer_service::metrics(&state, &admin_user, &query_filter).await?;
    assert_eq!(remaining_metrics.items.len(), 2);
    let remaining_logs = explorer_service::logs(&state, &admin_user, &query_filter).await?;
    assert_eq!(remaining_logs.items.len(), 2);

    // Test Reset "all" kinds
    let reset_all_count =
        explorer_service::reset_records(&state, &admin_user, "all", app_id, Some(env_id)).await?;
    assert_eq!(reset_all_count, 4); // 2 metrics + 2 logs

    let final_metrics = explorer_service::metrics(&state, &admin_user, &query_filter).await?;
    assert_eq!(final_metrics.items.len(), 0);
    let final_logs = explorer_service::logs(&state, &admin_user, &query_filter).await?;
    assert_eq!(final_logs.items.len(), 0);

    Ok(())
}

#[tokio::test]
async fn test_explorer_clean_invalid_data() -> Result<(), Box<dyn std::error::Error>> {
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

    let app_id = "test-clean-app";
    let env_id = "test-clean-env";
    let scope = IngestScope::for_test(app_id, env_id, "device-1");

    let events = vec![
        EventInput {
            name: "valid_event".into(),
            timestamp: None,
            anonymous_id: None,
            session_id: None,
            app_version: Some("1.0.0".into()),
            os: Some("Windows 11 Build 22631".into()),
            system_language: None,
            architecture: None,
            attributes: Attributes::default(),
            idempotency_key: None,
        },
        EventInput {
            name: "invalid_event_null_os".into(),
            timestamp: None,
            anonymous_id: None,
            session_id: None,
            app_version: None,
            os: None,
            system_language: None,
            architecture: None,
            attributes: Attributes::default(),
            idempotency_key: None,
        },
        EventInput {
            name: "invalid_event_unknown_os".into(),
            timestamp: None,
            anonymous_id: None,
            session_id: None,
            app_version: None,
            os: Some("unknown".into()),
            system_language: None,
            architecture: None,
            attributes: Attributes::default(),
            idempotency_key: None,
        },
    ];
    let ev_receipt = telemetry::events(&state, &scope, events).await?;
    assert_eq!(ev_receipt.accepted, 3);

    let admin_user = AuthenticatedUser {
        id: "user-admin".into(),
        email: "admin@test.com".into(),
        username: "admin".into(),
        locale: "zh-CN".into(),
        roles: vec!["Admin".into()],
        grants: vec![PermissionGrant {
            application_id: None,
            permissions: vec!["*".into()],
        }],
        totp_enabled: false,
    };

    // Test Preview clean invalid data
    let preview =
        explorer_service::preview_clean_invalid_data(&state, &admin_user, Some(app_id)).await?;
    assert_eq!(preview.invalid_events, 2);
    assert!(preview.total_invalid >= 2);
    assert!(!preview.samples.is_empty());
    assert!(preview.samples.iter().any(|s| s.kind == "events"));

    let result = explorer_service::clean_invalid_data(&state, &admin_user, Some(app_id)).await?;
    assert_eq!(result.deleted_events, 2);
    assert!(result.total_deleted >= 2);
    assert_eq!(result.deleted_events, preview.invalid_events);

    // Verify preview after cleaning is now zero
    let after_preview =
        explorer_service::preview_clean_invalid_data(&state, &admin_user, Some(app_id)).await?;
    assert_eq!(after_preview.invalid_events, 0);
    assert_eq!(after_preview.total_invalid, 0);
    assert!(after_preview.samples.is_empty());

    let query_filter = ServiceFilter {
        application_id: app_id.into(),
        environment_id: Some(env_id.into()),
        from: None,
        to: None,
        name: None,
        level: None,
        text: None,
        page: 1,
        page_size: 50,
    };
    let remaining_events = explorer_service::events(&state, &admin_user, &query_filter).await?;
    assert_eq!(remaining_events.items.len(), 1);
    assert_eq!(remaining_events.items[0].name, "valid_event");

    Ok(())
}

#[tokio::test]
async fn test_overview_filter_by_application() -> Result<(), Box<dyn std::error::Error>> {
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

    let app_a = "app-a";
    let app_b = "app-b";
    let env_id = "test-env";

    // Ingest events for app A
    let scope_a = IngestScope::for_test(app_a, env_id, "device-a");
    let events_a = vec![EventInput {
        name: "event_a".into(),
        timestamp: None,
        anonymous_id: None,
        session_id: None,
        app_version: Some("1.0.0".into()),
        os: Some("Windows 11 Build 22631".into()),
        system_language: None,
        architecture: None,
        attributes: Attributes::default(),
        idempotency_key: None,
    }];
    telemetry::events(&state, &scope_a, events_a).await?;

    // Ingest events for app B
    let scope_b = IngestScope::for_test(app_b, env_id, "device-b");
    let events_b = vec![
        EventInput {
            name: "event_b1".into(),
            timestamp: None,
            anonymous_id: None,
            session_id: None,
            app_version: Some("2.0.0".into()),
            os: Some("Linux (Ubuntu 24.04 Linux)".into()),
            system_language: None,
            architecture: None,
            attributes: Attributes::default(),
            idempotency_key: None,
        },
        EventInput {
            name: "event_b2".into(),
            timestamp: None,
            anonymous_id: None,
            session_id: None,
            app_version: Some("2.0.0".into()),
            os: Some("Linux (Ubuntu 24.04 Linux)".into()),
            system_language: None,
            architecture: None,
            attributes: Attributes::default(),
            idempotency_key: None,
        },
    ];
    telemetry::events(&state, &scope_b, events_b).await?;

    let admin_user = AuthenticatedUser {
        id: "user-admin".into(),
        email: "admin@test.com".into(),
        username: "admin".into(),
        locale: "zh-CN".into(),
        roles: vec!["Admin".into()],
        grants: vec![PermissionGrant {
            application_id: None,
            permissions: vec!["*".into()],
        }],
        totp_enabled: false,
    };

    // Global overview (all applications)
    let overview_all =
        sonde::services::statistics::overview(&state, &admin_user, None, Some(30)).await?;
    assert_eq!(overview_all.events_24h, 3);

    // Filter by App A
    let overview_a =
        sonde::services::statistics::overview(&state, &admin_user, Some(app_a), Some(30)).await?;
    assert_eq!(overview_a.events_24h, 1);

    // Filter by App B
    let overview_b =
        sonde::services::statistics::overview(&state, &admin_user, Some(app_b), Some(30)).await?;
    assert_eq!(overview_b.events_24h, 2);

    Ok(())
}

#[tokio::test]
async fn test_cross_application_permission_isolation() -> Result<(), Box<dyn std::error::Error>> {
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

    // Admin A creates App A
    let (app_a, env_a) =
        database::applications::create_application(&database, "App A", "app-a", Some("user-a"))
            .await?;

    // Admin B creates App B
    let (app_b, env_b) =
        database::applications::create_application(&database, "App B", "app-b", Some("user-b"))
            .await?;

    // Ingest event for App A
    let scope_a = IngestScope::for_test(&app_a, &env_a, "device-a");
    telemetry::events(
        &state,
        &scope_a,
        vec![EventInput {
            name: "event_a".into(),
            timestamp: None,
            anonymous_id: None,
            session_id: None,
            app_version: Some("1.0.0".into()),
            os: Some("Windows 11".into()),
            system_language: None,
            architecture: None,
            attributes: Attributes::default(),
            idempotency_key: None,
        }],
    )
    .await?;

    // Ingest event for App B
    let scope_b = IngestScope::for_test(&app_b, &env_b, "device-b");
    telemetry::events(
        &state,
        &scope_b,
        vec![EventInput {
            name: "event_b".into(),
            timestamp: None,
            anonymous_id: None,
            session_id: None,
            app_version: Some("2.0.0".into()),
            os: Some("macOS".into()),
            system_language: None,
            architecture: None,
            attributes: Attributes::default(),
            idempotency_key: None,
        }],
    )
    .await?;

    // User A: Owner of App A, but has NO permissions on App B, and NO unscoped global admin role
    let user_a = AuthenticatedUser {
        id: "user-a".into(),
        email: "usera@test.com".into(),
        username: "usera".into(),
        locale: "zh-CN".into(),
        roles: vec!["User".into()],
        grants: vec![PermissionGrant {
            application_id: Some(app_a.clone()),
            permissions: vec![
                "apps.read".into(),
                "apps.manage".into(),
                "telemetry.read".into(),
            ],
        }],
        totp_enabled: false,
    };

    // 1. User A can view App A's overview and explorer events
    let filter_a = ServiceFilter {
        application_id: app_a.clone(),
        environment_id: Some(env_a.clone()),
        from: None,
        to: None,
        name: None,
        level: None,
        text: None,
        page: 1,
        page_size: 10,
    };
    let events_a = explorer_service::events(&state, &user_a, &filter_a).await?;
    assert_eq!(events_a.items.len(), 1);

    let overview_a =
        sonde::services::statistics::overview(&state, &user_a, Some(&app_a), Some(30)).await?;
    assert_eq!(overview_a.events_24h, 1);

    // 2. User A attempting to view App B's explorer events MUST be forbidden
    let filter_b = ServiceFilter {
        application_id: app_b.clone(),
        environment_id: Some(env_b.clone()),
        from: None,
        to: None,
        name: None,
        level: None,
        text: None,
        page: 1,
        page_size: 10,
    };
    let err_b_events = explorer_service::events(&state, &user_a, &filter_b).await;
    assert!(err_b_events.is_err(), "User A must not read App B events");

    // 3. User A attempting to view App B's statistics overview MUST be forbidden
    let err_b_overview =
        sonde::services::statistics::overview(&state, &user_a, Some(&app_b), Some(30)).await;
    assert!(err_b_overview.is_err(), "User A must not read App B stats");

    // 4. User A attempting to view global overview (None) MUST be forbidden (no unscoped telemetry.read)
    let err_global_overview =
        sonde::services::statistics::overview(&state, &user_a, None, Some(30)).await;
    assert!(
        err_global_overview.is_err(),
        "User A must not read global overview"
    );

    // 5. User A attempting to reset or delete records from App B MUST be forbidden
    let err_b_reset =
        explorer_service::reset_records(&state, &user_a, "events", &app_b, Some(&env_b)).await;
    assert!(err_b_reset.is_err(), "User A must not reset App B records");

    // 6. User A attempting to clean invalid data from App B MUST be forbidden
    let err_b_clean = explorer_service::clean_invalid_data(&state, &user_a, Some(&app_b)).await;
    assert!(
        err_b_clean.is_err(),
        "User A must not clean App B invalid data"
    );

    // 7. User A attempting to clean invalid data globally (None) MUST be forbidden
    let err_global_clean = explorer_service::clean_invalid_data(&state, &user_a, None).await;
    assert!(
        err_global_clean.is_err(),
        "User A must not clean invalid data globally"
    );

    // 8. User A CAN clean invalid data for their own App A
    let clean_a_ok = explorer_service::clean_invalid_data(&state, &user_a, Some(&app_a)).await;
    assert!(
        clean_a_ok.is_ok(),
        "User A must be allowed to clean App A invalid data"
    );

    // 9. User A attempting to list alerts in App B MUST be forbidden
    let err_b_alerts = sonde::services::alerts::list(&state, &user_a, Some(&app_b)).await;
    assert!(err_b_alerts.is_err(), "User A must not list App B alerts");

    // 10. User A attempting to list error groups in App B MUST be forbidden
    let filter_b_errors = sonde::services::errors::ErrorGroupFilter {
        application_id: app_b.clone(),
        environment_id: None,
        severity: None,
        from: None,
        to: None,
        page: 1,
        page_size: 10,
    };
    let err_b_errors = sonde::services::errors::groups(&state, &user_a, &filter_b_errors).await;
    assert!(
        err_b_errors.is_err(),
        "User A must not list App B error groups"
    );

    Ok(())
}
