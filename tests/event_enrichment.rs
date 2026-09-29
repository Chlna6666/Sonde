#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use sonde::{
    config::{InstallationConfig, MasterKey},
    database::{self, explorer::ExplorerFilter},
    domain::{device_facts::DeviceFactsInput, telemetry::EventInput},
    security::AuthSecurity,
    services::telemetry::{self, IngestScope},
    state::InstalledState,
};

#[tokio::test]
async fn server_enriches_events_with_recorded_device_facts()
-> Result<(), Box<dyn std::error::Error>> {
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

    let app_id = "test-app-enrichment";
    let env_id = "test-env-enrichment";
    let device_id = "device-test-enrichment-1234";

    let ingest_scope = IngestScope::for_test(app_id, env_id, device_id);

    // 1. Device reports facts via heartbeat
    telemetry::heartbeat(
        &state,
        &ingest_scope,
        DeviceFactsInput {
            app_version: Some("1.2.3".into()),
            launcher_version: Some("0.4.0".into()),
            os: Some("windows".into()),
            system_language: Some("zh-CN".into()),
            architecture: Some("x86_64".into()),
        },
    )
    .await?;

    // 2. Client sends an event where facts are None (like previous version or minimal payload)
    let event = EventInput {
        name: "test_event".into(),
        timestamp: None,
        anonymous_id: None,
        session_id: None,
        app_version: None,
        launcher_version: None,
        os: None,
        system_language: None,
        architecture: None,
        idempotency_key: None,
        attributes: Default::default(),
    };

    let receipt = telemetry::events(&state, &ingest_scope, vec![event]).await?;
    assert_eq!(receipt.accepted, 1);
    assert!(receipt.rejected.is_empty());

    // Allow ingest writer queue to flush
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // 3. Query via explorer and verify facts are enriched
    let filter = ExplorerFilter {
        application_id: app_id.into(),
        environment_id: Some(env_id.into()),
        from: None,
        to: None,
        name: None,
        level: None,
        text: None,
        page: 1,
        page_size: 10,
    };

    let page = database::explorer::events(&database, &filter).await?;
    assert_eq!(page.items.len(), 1);
    let record = &page.items[0];
    assert_eq!(record.name, "test_event");
    assert_eq!(record.app_version.as_deref(), Some("1.2.3"));
    assert_eq!(record.launcher_version.as_deref(), Some("0.4.0"));
    assert_eq!(record.os.as_deref(), Some("windows"));

    Ok(())
}
