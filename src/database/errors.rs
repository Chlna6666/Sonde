use std::collections::HashMap;

use sea_orm::{
    ConnectionTrait, DbErr,
    sea_query::{Alias, Expr, ExprTrait, Query, Value},
};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::domain::telemetry::{ErrorInput, ErrorSeverity};

use super::{
    query::{insert_batch, insert_batch_ignore_conflicts},
    telemetry::TelemetryScope,
};

#[derive(Debug)]
struct ErrorGroupBatch {
    id: String,
    fingerprint: String,
    name: String,
    message_sample: String,
    severity: String,
    first_seen: i64,
    last_seen: i64,
    occurrences: i64,
    last_app_version: Option<String>,
    last_launcher_version: Option<String>,
    last_os: Option<String>,
}

pub async fn insert_error_index(
    database: &impl ConnectionTrait,
    scope: &TelemetryScope,
    errors: &[ErrorInput],
    received_at: i64,
) -> Result<(), DbErr> {
    if errors.is_empty() {
        return Ok(());
    }

    let mut groups: HashMap<String, ErrorGroupBatch> = HashMap::new();
    let mut occurrence_rows = Vec::with_capacity(errors.len());

    for error in errors {
        let timestamp = error.timestamp.unwrap_or(received_at);
        let fingerprint = error_fingerprint(error);
        let group_id = error_group_id(scope, &fingerprint);
        let severity = severity_name(error.severity.as_ref()).to_owned();
        let anonymous_id = error
            .anonymous_id
            .as_deref()
            .map(|value| anonymous_hash(scope, value));
        let handled = error.handled.map(|value| if value { 1_i64 } else { 0_i64 });

        let group = groups.entry(group_id.clone()).or_insert_with(|| ErrorGroupBatch {
            id: group_id.clone(),
            fingerprint: fingerprint.clone(),
            name: error.name.clone(),
            message_sample: error.message.clone(),
            severity: severity.clone(),
            first_seen: timestamp,
            last_seen: timestamp,
            occurrences: 0,
            last_app_version: error.app_version.clone(),
            last_launcher_version: error.launcher_version.clone(),
            last_os: error.os.clone(),
        });
        group.first_seen = std::cmp::min(group.first_seen, timestamp);
        if timestamp >= group.last_seen {
            group.last_seen = timestamp;
            group.message_sample.clone_from(&error.message);
            group.severity.clone_from(&severity);
            group.last_app_version.clone_from(&error.app_version);
            group.last_launcher_version.clone_from(&error.launcher_version);
            group.last_os.clone_from(&error.os);
        }
        group.occurrences = group.occurrences.saturating_add(1);

        occurrence_rows.push(vec![
            Value::from(Uuid::now_v7().to_string()),
            Value::from(group_id),
            Value::from(scope.application_id.clone()),
            Value::from(scope.environment_id.clone()),
            Value::from(timestamp),
            Value::from(anonymous_id),
            Value::from(error.session_id.clone()),
            Value::from(error.app_version.clone()),
            Value::from(error.launcher_version.clone()),
            Value::from(error.os.clone()),
            Value::from(error.stack_trace.clone()),
            Value::from(handled),
            Value::from(serde_json::to_string(&error.attributes).map_err(json_error)?),
            Value::from(received_at),
        ]);
    }

    let group_columns = [
        "id",
        "application_id",
        "environment_id",
        "fingerprint",
        "name",
        "message_sample",
        "severity",
        "first_seen",
        "last_seen",
        "occurrences",
        "last_app_version",
        "last_launcher_version",
        "last_os",
        "updated_at",
    ];
    let group_rows = groups
        .values()
        .map(|group| {
            vec![
                Value::from(group.id.clone()),
                Value::from(scope.application_id.clone()),
                Value::from(scope.environment_id.clone()),
                Value::from(group.fingerprint.clone()),
                Value::from(group.name.clone()),
                Value::from(group.message_sample.clone()),
                Value::from(group.severity.clone()),
                Value::from(group.first_seen),
                Value::from(group.last_seen),
                Value::from(0_i64),
                Value::from(group.last_app_version.clone()),
                Value::from(group.last_launcher_version.clone()),
                Value::from(group.last_os.clone()),
                Value::from(received_at),
            ]
        })
        .collect();
    insert_batch_ignore_conflicts(
        database,
        "error_groups",
        &group_columns,
        group_rows,
        "id",
        "id",
    )
    .await?;

    for group in groups.values() {
        let query = Query::update()
            .table(Alias::new("error_groups"))
            .value(
                Alias::new("occurrences"),
                Expr::col(Alias::new("occurrences")).add(group.occurrences),
            )
            .value(
                Alias::new("first_seen"),
                Expr::cust_with_values(
                    "CASE WHEN first_seen > ? THEN ? ELSE first_seen END",
                    [group.first_seen, group.first_seen],
                ),
            )
            .value(
                Alias::new("last_seen"),
                Expr::cust_with_values(
                    "CASE WHEN last_seen < ? THEN ? ELSE last_seen END",
                    [group.last_seen, group.last_seen],
                ),
            )
            .value(Alias::new("message_sample"), group.message_sample.clone())
            .value(Alias::new("severity"), group.severity.clone())
            .value(
                Alias::new("last_app_version"),
                group.last_app_version.clone(),
            )
            .value(
                Alias::new("last_launcher_version"),
                group.last_launcher_version.clone(),
            )
            .value(Alias::new("last_os"), group.last_os.clone())
            .value(Alias::new("updated_at"), received_at)
            .and_where(Expr::col(Alias::new("id")).eq(&group.id))
            .to_owned();
        database.execute(&query).await?;
    }

    let occurrence_columns = [
        "id",
        "group_id",
        "application_id",
        "environment_id",
        "timestamp",
        "anonymous_id",
        "session_id",
        "app_version",
        "launcher_version",
        "os",
        "stack_trace",
        "handled",
        "attributes",
        "received_at",
    ];
    insert_batch(
        database,
        "error_occurrences",
        &occurrence_columns,
        occurrence_rows,
    )
    .await?;

    Ok(())
}

fn severity_name(severity: Option<&ErrorSeverity>) -> &'static str {
    match severity {
        Some(ErrorSeverity::Fatal) => "fatal",
        Some(ErrorSeverity::Warning) => "warning",
        _ => "error",
    }
}

fn error_fingerprint(error: &ErrorInput) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"sonde:error-fingerprint:v1\0");
    hasher.update(error.name.as_bytes());
    hasher.update(b"\0");
    if let Some(stack_trace) = error.stack_trace.as_deref() {
        hasher.update(stable_prefix(stack_trace, 4_096).as_bytes());
    } else {
        hasher.update(stable_prefix(&error.message, 512).as_bytes());
    }
    hex::encode(hasher.finalize())
}

fn error_group_id(scope: &TelemetryScope, fingerprint: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"sonde:error-group:v1\0");
    hasher.update(scope.application_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(scope.environment_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(fingerprint.as_bytes());
    format!("eg_{}", hex::encode(hasher.finalize()))
}

fn anonymous_hash(scope: &TelemetryScope, value: &str) -> String {
    let salt = format!("{}:{}", scope.application_id, scope.environment_id);
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hasher.update(salt.as_bytes());
    hex::encode(hasher.finalize())
}

fn stable_prefix(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn json_error(error: serde_json::Error) -> DbErr {
    DbErr::Custom(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::domain::telemetry::ErrorInput;

    use super::error_fingerprint;

    #[test]
    fn fingerprint_is_stable_and_stack_based() {
        let mut error = ErrorInput {
            name: "panic".into(),
            message: "dynamic message 1".into(),
            stack_trace: Some("frame_a\nframe_b".into()),
            severity: None,
            handled: Some(false),
            timestamp: None,
            anonymous_id: None,
            session_id: None,
            app_version: None,
            launcher_version: None,
            os: None,
            attributes: BTreeMap::new(),
        };
        let first = error_fingerprint(&error);
        error.message = "dynamic message 2".into();
        assert_eq!(first, error_fingerprint(&error));
        error.stack_trace = Some("frame_c".into());
        assert_ne!(first, error_fingerprint(&error));
    }
}
