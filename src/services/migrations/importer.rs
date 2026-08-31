use std::collections::BTreeMap;

use serde::Serialize;

use crate::{
    database::{imports, telemetry},
    domain::telemetry::EventInput,
    error::AppError,
    services::authentication::AuthenticatedUser,
    state::InstalledState,
};

use super::parser::parse_d1_export;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImportResult {
    pub run: imports::ImportRun,
    pub already_imported: bool,
}

pub(crate) async fn execute_d1_import(
    installed: &InstalledState,
    sql: &str,
    application_id: &str,
    environment_id: &str,
) -> Result<ImportResult, AppError> {
    if !imports::scope_exists(&installed.database, application_id, environment_id).await? {
        return Err(AppError::Validation(
            "the selected application environment does not exist".into(),
        ));
    }
    let parsed = parse_d1_export(sql)?;
    if let Some(run) =
        imports::find_by_source_hash(&installed.database, &parsed.preview.source_hash).await?
    {
        return Ok(ImportResult {
            run,
            already_imported: true,
        });
    }

    let scope = telemetry::TelemetryScope {
        application_id: application_id.to_owned(),
        environment_id: environment_id.to_owned(),
    };
    let (inserted, deduped) = import_rows(
        installed,
        &scope,
        application_id,
        parsed.rows,
        parsed.preview.duplicates as i64,
    )
    .await?;
    let run = imports::ImportRun {
        id: uuid::Uuid::now_v7().to_string(),
        source_hash: parsed.preview.source_hash,
        application_id: application_id.to_owned(),
        environment_id: environment_id.to_owned(),
        status: "completed".into(),
        inserted,
        deduped,
        rejected: parsed.preview.rejected as i64,
        created_at: chrono::Utc::now().timestamp_millis(),
    };
    imports::create(&installed.database, &run).await?;
    Ok(ImportResult {
        run,
        already_imported: false,
    })
}

pub(crate) async fn list_runs(
    installed: &InstalledState,
    user: &AuthenticatedUser,
) -> Result<Vec<imports::ImportRun>, AppError> {
    user.require("migrations.manage", None)?;
    Ok(imports::list(&installed.database).await?)
}

pub(super) async fn import_rows(
    installed: &InstalledState,
    scope: &telemetry::TelemetryScope,
    application_id: &str,
    rows: Vec<super::parser::LegacyEventRow>,
    initial_duplicates: i64,
) -> Result<(i64, i64), AppError> {
    let mut inserted = 0_i64;
    let mut deduped = initial_duplicates;
    for row in rows {
        let dedupe_key = format!("d1:{application_id}:{}:{}", row.day, row.user_hash);
        let event = migrated_event(row);
        if telemetry::insert_migrated_event(&installed.database, scope, &event, &dedupe_key).await? {
            inserted += 1;
        } else {
            deduped += 1;
        }
    }
    Ok((inserted, deduped))
}

fn migrated_event(row: super::parser::LegacyEventRow) -> EventInput {
    let mut attributes = BTreeMap::new();
    attributes.insert(
        "migration.source_day".into(),
        serde_json::Value::String(row.day),
    );
    attributes.insert(
        "migration.source_timestamp".into(),
        serde_json::Value::from(row.ts),
    );
    attributes.insert(
        "migration.source_user_hash".into(),
        serde_json::Value::String(row.user_hash.clone()),
    );
    if let Some(key_id) = row.key_id {
        attributes.insert(
            "migration.source_key_id".into(),
            serde_json::Value::String(key_id),
        );
    }
    EventInput {
        name: "migration.application_start".into(),
        timestamp: Some(row.ts),
        anonymous_id: Some(row.user_hash),
        session_id: None,
        app_version: row.app_version,
        launcher_version: row.launcher_version,
        os: row.os,
        idempotency_key: None,
        attributes,
    }
}
