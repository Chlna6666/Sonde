use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, DbErr, TransactionTrait,
    sea_query::{Alias, Expr, ExprTrait, LockType, Query},
};
use uuid::Uuid;

use super::{device_activity, query::insert_batch_ignore_conflicts, telemetry::TelemetryScope};

const DAY_MILLIS: i64 = 86_400_000;
const SESSION_IDLE_MILLIS: i64 = 30 * 60_000;
const RAPID_VERSION_MILLIS: i64 = 5 * 60_000;
const RAPID_OS_MILLIS: i64 = 60 * 60_000;

#[derive(Clone, Copy, Debug)]
pub enum DeviceTelemetryKind {
    Heartbeat,
    Event,
    Metric,
    Log,
    Error,
}

#[derive(Clone, Debug)]
pub struct TimedDimension {
    pub value: String,
    pub timestamp: i64,
}

#[derive(Clone, Debug)]
pub struct DeviceObservation {
    pub kind: DeviceTelemetryKind,
    pub received_at: i64,
    pub telemetry_at: i64,
    pub item_count: usize,
    pub session_id: Option<TimedDimension>,
    pub app_version: Option<TimedDimension>,
    pub launcher_version: Option<TimedDimension>,
    pub os: Option<TimedDimension>,
    pub system_language: Option<TimedDimension>,
    pub architecture: Option<TimedDimension>,
}

#[derive(Debug)]
struct DeviceRow {
    last_seen_at: i64,
    last_event_at: Option<i64>,
    last_metric_at: Option<i64>,
    last_log_at: Option<i64>,
    last_error_at: Option<i64>,
    last_session_id: Option<String>,
    last_session_at: Option<i64>,
    last_app_version: Option<String>,
    last_app_version_at: Option<i64>,
    last_launcher_version: Option<String>,
    last_launcher_version_at: Option<i64>,
    last_os: Option<String>,
    last_os_at: Option<i64>,
    last_system_language: Option<String>,
    last_system_language_at: Option<i64>,
    last_architecture: Option<String>,
    last_architecture_at: Option<i64>,
    event_items: i64,
    metric_items: i64,
    log_items: i64,
    error_items: i64,
    session_changes: i64,
    app_version_changes: i64,
    launcher_version_changes: i64,
    os_changes: i64,
    risk_score: i32,
    last_anomaly: Option<String>,
    last_anomaly_at: Option<i64>,
}

#[derive(Debug)]
struct DimensionMerge {
    value: Option<String>,
    timestamp: Option<i64>,
    changed: bool,
    change_interval: Option<i64>,
}

#[derive(Debug)]
struct SessionState {
    id: String,
    last_activity_at: i64,
    started_new: bool,
}

#[derive(Debug)]
struct TelemetryCounters {
    last_event_at: Option<i64>,
    last_metric_at: Option<i64>,
    last_log_at: Option<i64>,
    last_error_at: Option<i64>,
    event_items: i64,
    metric_items: i64,
    log_items: i64,
    error_items: i64,
}

/// Update the derived, server-owned device profile after authoritative telemetry has been stored.
///
/// Raw telemetry remains the source of truth. This profile is an operational index for current
/// state, activity and abuse signals. Session boundaries are derived exclusively from server receive
/// time so a client cannot inflate session counts or durations by supplying arbitrary session IDs.
pub async fn observe(
    database: &DatabaseConnection,
    scope: &TelemetryScope,
    device_hash: &str,
    observation: &DeviceObservation,
) -> Result<(), DbErr> {
    let transaction = database.begin().await?;
    ensure_device_row(
        &transaction,
        scope,
        device_hash,
        observation.received_at,
    )
    .await?;

    let current = load_device_for_update(&transaction, device_hash)
        .await?
        .ok_or_else(|| DbErr::Custom("device profile row disappeared during update".into()))?;
    let counters = update_counters(&current, observation);
    device_activity::record(
        &transaction,
        scope,
        device_hash,
        current.last_seen_at,
        observation.received_at,
    )
    .await?;
    let session = derive_session(
        current.last_session_id,
        current.last_seen_at,
        observation.received_at,
    );
    let app_version = merge_dimension(
        current.last_app_version,
        current.last_app_version_at,
        observation.app_version.as_ref(),
    );
    let launcher_version = merge_dimension(
        current.last_launcher_version,
        current.last_launcher_version_at,
        observation.launcher_version.as_ref(),
    );
    let os = merge_dimension(
        current.last_os,
        current.last_os_at,
        observation.os.as_ref(),
    );
    let system_language = merge_dimension(
        current.last_system_language,
        current.last_system_language_at,
        observation.system_language.as_ref(),
    );
    let architecture = merge_dimension(
        current.last_architecture,
        current.last_architecture_at,
        observation.architecture.as_ref(),
    );

    let mut anomaly_flags = Vec::new();
    let elapsed_days = observation
        .received_at
        .saturating_sub(current.last_seen_at)
        .div_euclid(DAY_MILLIS)
        .clamp(0, i32::MAX as i64) as i32;
    let mut risk_score = std::cmp::max(current.risk_score.saturating_sub(elapsed_days), 0);

    if observation.item_count >= 750 {
        add_risk(&mut risk_score, 5, &mut anomaly_flags, "large_batch");
    }
    let age = observation
        .received_at
        .saturating_sub(observation.telemetry_at);
    if age > DAY_MILLIS {
        add_risk(&mut risk_score, 3, &mut anomaly_flags, "late_telemetry");
    }
    if observation.telemetry_at > observation.received_at.saturating_add(60_000) {
        add_risk(&mut risk_score, 3, &mut anomaly_flags, "clock_ahead");
    }
    if app_version.changed
        && app_version
            .change_interval
            .is_some_and(|interval| interval <= RAPID_VERSION_MILLIS)
    {
        add_risk(
            &mut risk_score,
            8,
            &mut anomaly_flags,
            "rapid_app_version_change",
        );
    }
    if launcher_version.changed
        && launcher_version
            .change_interval
            .is_some_and(|interval| interval <= RAPID_VERSION_MILLIS)
    {
        add_risk(
            &mut risk_score,
            6,
            &mut anomaly_flags,
            "rapid_launcher_version_change",
        );
    }
    if os.changed {
        add_risk(&mut risk_score, 15, &mut anomaly_flags, "os_changed");
        if os
            .change_interval
            .is_some_and(|interval| interval <= RAPID_OS_MILLIS)
        {
            add_risk(&mut risk_score, 10, &mut anomaly_flags, "rapid_os_change");
        }
    }

    let (last_anomaly, last_anomaly_at) = if anomaly_flags.is_empty() {
        (current.last_anomaly, current.last_anomaly_at)
    } else {
        (Some(anomaly_flags.join(",")), Some(observation.received_at))
    };

    let update = Query::update()
        .table(Alias::new("telemetry_devices"))
        .value(
            Alias::new("last_seen_at"),
            std::cmp::max(current.last_seen_at, observation.received_at),
        )
        .value(Alias::new("last_event_at"), counters.last_event_at)
        .value(Alias::new("last_metric_at"), counters.last_metric_at)
        .value(Alias::new("last_log_at"), counters.last_log_at)
        .value(Alias::new("last_error_at"), counters.last_error_at)
        .value(Alias::new("last_session_id"), session.id)
        .value(Alias::new("last_session_at"), session.last_activity_at)
        .value(Alias::new("last_app_version"), app_version.value)
        .value(Alias::new("last_app_version_at"), app_version.timestamp)
        .value(
            Alias::new("last_launcher_version"),
            launcher_version.value,
        )
        .value(
            Alias::new("last_launcher_version_at"),
            launcher_version.timestamp,
        )
        .value(Alias::new("last_os"), os.value)
        .value(Alias::new("last_os_at"), os.timestamp)
        .value(
            Alias::new("last_system_language"),
            system_language.value,
        )
        .value(
            Alias::new("last_system_language_at"),
            system_language.timestamp,
        )
        .value(Alias::new("last_architecture"), architecture.value)
        .value(
            Alias::new("last_architecture_at"),
            architecture.timestamp,
        )
        .value(Alias::new("event_items"), counters.event_items)
        .value(Alias::new("metric_items"), counters.metric_items)
        .value(Alias::new("log_items"), counters.log_items)
        .value(Alias::new("error_items"), counters.error_items)
        .value(
            Alias::new("session_changes"),
            current
                .session_changes
                .saturating_add(change_increment(session.started_new)),
        )
        .value(
            Alias::new("app_version_changes"),
            current
                .app_version_changes
                .saturating_add(change_increment(app_version.changed)),
        )
        .value(
            Alias::new("launcher_version_changes"),
            current
                .launcher_version_changes
                .saturating_add(change_increment(launcher_version.changed)),
        )
        .value(
            Alias::new("os_changes"),
            current
                .os_changes
                .saturating_add(change_increment(os.changed)),
        )
        .value(Alias::new("risk_score"), risk_score)
        .value(Alias::new("last_anomaly"), last_anomaly)
        .value(Alias::new("last_anomaly_at"), last_anomaly_at)
        .value(Alias::new("updated_at"), observation.received_at)
        .and_where(Expr::col(Alias::new("id")).eq(device_hash))
        .to_owned();
    transaction.execute(&update).await?;
    transaction.commit().await
}

async fn ensure_device_row(
    database: &impl ConnectionTrait,
    scope: &TelemetryScope,
    device_hash: &str,
    now: i64,
) -> Result<(), DbErr> {
    insert_batch_ignore_conflicts(
        database,
        "telemetry_devices",
        &[
            "id",
            "application_id",
            "environment_id",
            "device_hash",
            "first_seen_at",
            "last_seen_at",
            "last_event_at",
            "last_metric_at",
            "last_log_at",
            "last_error_at",
            "last_session_id",
            "last_session_at",
            "last_app_version",
            "last_app_version_at",
            "last_launcher_version",
            "last_launcher_version_at",
            "last_os",
            "last_os_at",
            "last_system_language",
            "last_system_language_at",
            "last_architecture",
            "last_architecture_at",
            "event_items",
            "metric_items",
            "log_items",
            "error_items",
            "session_changes",
            "app_version_changes",
            "launcher_version_changes",
            "os_changes",
            "risk_score",
            "last_anomaly",
            "last_anomaly_at",
            "updated_at",
        ],
        vec![vec![
            device_hash.to_owned().into(),
            scope.application_id.clone().into(),
            scope.environment_id.clone().into(),
            device_hash.to_owned().into(),
            now.into(),
            now.into(),
            Option::<i64>::None.into(),
            Option::<i64>::None.into(),
            Option::<i64>::None.into(),
            Option::<i64>::None.into(),
            Option::<String>::None.into(),
            Option::<i64>::None.into(),
            Option::<String>::None.into(),
            Option::<i64>::None.into(),
            Option::<String>::None.into(),
            Option::<i64>::None.into(),
            Option::<String>::None.into(),
            Option::<i64>::None.into(),
            Option::<String>::None.into(),
            Option::<i64>::None.into(),
            Option::<String>::None.into(),
            Option::<i64>::None.into(),
            0_i64.into(),
            0_i64.into(),
            0_i64.into(),
            0_i64.into(),
            0_i64.into(),
            0_i64.into(),
            0_i64.into(),
            0_i64.into(),
            0_i32.into(),
            Option::<String>::None.into(),
            Option::<i64>::None.into(),
            now.into(),
        ]],
        "id",
        "id",
    )
    .await?;
    Ok(())
}

async fn load_device_for_update(
    database: &impl ConnectionTrait,
    device_hash: &str,
) -> Result<Option<DeviceRow>, DbErr> {
    let mut query = Query::select();
    query
        .columns(
            [
                "last_seen_at",
                "last_event_at",
                "last_metric_at",
                "last_log_at",
                "last_error_at",
                "last_session_id",
                "last_session_at",
                "last_app_version",
                "last_app_version_at",
                "last_launcher_version",
                "last_launcher_version_at",
                "last_os",
                "last_os_at",
                "last_system_language",
                "last_system_language_at",
                "last_architecture",
                "last_architecture_at",
                "event_items",
                "metric_items",
                "log_items",
                "error_items",
                "session_changes",
                "app_version_changes",
                "launcher_version_changes",
                "os_changes",
                "risk_score",
                "last_anomaly",
                "last_anomaly_at",
            ]
            .map(Alias::new),
        )
        .from(Alias::new("telemetry_devices"))
        .and_where(Expr::col(Alias::new("id")).eq(device_hash))
        .limit(1);
    if database.get_database_backend() != DbBackend::Sqlite {
        query.lock(LockType::Update);
    }
    let query = query.to_owned();
    database
        .query_one(&query)
        .await?
        .map(|row| {
            Ok(DeviceRow {
                last_seen_at: row.try_get("", "last_seen_at")?,
                last_event_at: row.try_get("", "last_event_at")?,
                last_metric_at: row.try_get("", "last_metric_at")?,
                last_log_at: row.try_get("", "last_log_at")?,
                last_error_at: row.try_get("", "last_error_at")?,
                last_session_id: row.try_get("", "last_session_id")?,
                last_session_at: row.try_get("", "last_session_at")?,
                last_app_version: row.try_get("", "last_app_version")?,
                last_app_version_at: row.try_get("", "last_app_version_at")?,
                last_launcher_version: row.try_get("", "last_launcher_version")?,
                last_launcher_version_at: row.try_get("", "last_launcher_version_at")?,
                last_os: row.try_get("", "last_os")?,
                last_os_at: row.try_get("", "last_os_at")?,
                last_system_language: row.try_get("", "last_system_language")?,
                last_system_language_at: row.try_get("", "last_system_language_at")?,
                last_architecture: row.try_get("", "last_architecture")?,
                last_architecture_at: row.try_get("", "last_architecture_at")?,
                event_items: row.try_get("", "event_items")?,
                metric_items: row.try_get("", "metric_items")?,
                log_items: row.try_get("", "log_items")?,
                error_items: row.try_get("", "error_items")?,
                session_changes: row.try_get("", "session_changes")?,
                app_version_changes: row.try_get("", "app_version_changes")?,
                launcher_version_changes: row.try_get("", "launcher_version_changes")?,
                os_changes: row.try_get("", "os_changes")?,
                risk_score: row.try_get("", "risk_score")?,
                last_anomaly: row.try_get("", "last_anomaly")?,
                last_anomaly_at: row.try_get("", "last_anomaly_at")?,
            })
        })
        .transpose()
}

fn update_counters(current: &DeviceRow, observation: &DeviceObservation) -> TelemetryCounters {
    let item_count = i64::try_from(observation.item_count).unwrap_or(i64::MAX);
    let mut counters = TelemetryCounters {
        last_event_at: current.last_event_at,
        last_metric_at: current.last_metric_at,
        last_log_at: current.last_log_at,
        last_error_at: current.last_error_at,
        event_items: current.event_items,
        metric_items: current.metric_items,
        log_items: current.log_items,
        error_items: current.error_items,
    };
    match observation.kind {
        DeviceTelemetryKind::Heartbeat => {}
        DeviceTelemetryKind::Event => {
            counters.last_event_at = max_timestamp(counters.last_event_at, observation.received_at);
            counters.event_items = counters.event_items.saturating_add(item_count);
        }
        DeviceTelemetryKind::Metric => {
            counters.last_metric_at = max_timestamp(counters.last_metric_at, observation.received_at);
            counters.metric_items = counters.metric_items.saturating_add(item_count);
        }
        DeviceTelemetryKind::Log => {
            counters.last_log_at = max_timestamp(counters.last_log_at, observation.received_at);
            counters.log_items = counters.log_items.saturating_add(item_count);
        }
        DeviceTelemetryKind::Error => {
            counters.last_error_at = max_timestamp(counters.last_error_at, observation.received_at);
            counters.error_items = counters.error_items.saturating_add(item_count);
        }
    }
    counters
}

fn derive_session(
    current_id: Option<String>,
    last_seen_at: i64,
    received_at: i64,
) -> SessionState {
    let gap = received_at.saturating_sub(last_seen_at);
    let had_current = current_id.is_some();
    if let Some(id) = current_id
        && gap <= SESSION_IDLE_MILLIS
    {
        return SessionState {
            id,
            last_activity_at: received_at,
            started_new: false,
        };
    }

    SessionState {
        id: Uuid::now_v7().to_string(),
        last_activity_at: received_at,
        started_new: had_current,
    }
}

fn merge_dimension(
    current_value: Option<String>,
    current_timestamp: Option<i64>,
    incoming: Option<&TimedDimension>,
) -> DimensionMerge {
    let Some(incoming) = incoming else {
        return DimensionMerge {
            value: current_value,
            timestamp: current_timestamp,
            changed: false,
            change_interval: None,
        };
    };
    if current_timestamp.is_some_and(|timestamp| incoming.timestamp < timestamp) {
        return DimensionMerge {
            value: current_value,
            timestamp: current_timestamp,
            changed: false,
            change_interval: None,
        };
    }

    let changed = current_value
        .as_deref()
        .is_some_and(|value| value != incoming.value.as_str());
    let change_interval = if changed {
        current_timestamp.map(|timestamp| {
            std::cmp::max(incoming.timestamp.saturating_sub(timestamp), 0)
        })
    } else {
        None
    };
    DimensionMerge {
        value: Some(incoming.value.clone()),
        timestamp: Some(incoming.timestamp),
        changed,
        change_interval,
    }
}

fn max_timestamp(current: Option<i64>, incoming: i64) -> Option<i64> {
    Some(current.map_or(incoming, |value| std::cmp::max(value, incoming)))
}

fn change_increment(changed: bool) -> i64 {
    if changed { 1 } else { 0 }
}

fn add_risk(
    score: &mut i32,
    delta: i32,
    flags: &mut Vec<&'static str>,
    flag: &'static str,
) {
    *score = std::cmp::min(score.saturating_add(delta), 100);
    flags.push(flag);
}

#[cfg(test)]
mod tests {
    use super::{
        DeviceObservation, DeviceRow, DeviceTelemetryKind, SESSION_IDLE_MILLIS, TimedDimension,
        derive_session, merge_dimension, update_counters,
    };

    #[test]
    fn older_dimension_observation_cannot_roll_back_current_state() {
        let merged = merge_dimension(
            Some("2.0.0".into()),
            Some(200),
            Some(&TimedDimension {
                value: "1.0.0".into(),
                timestamp: 100,
            }),
        );
        assert_eq!(merged.value.as_deref(), Some("2.0.0"));
        assert_eq!(merged.timestamp, Some(200));
        assert!(!merged.changed);
    }

    #[test]
    fn newer_dimension_change_tracks_interval() {
        let merged = merge_dimension(
            Some("windows".into()),
            Some(100),
            Some(&TimedDimension {
                value: "linux".into(),
                timestamp: 250,
            }),
        );
        assert_eq!(merged.value.as_deref(), Some("linux"));
        assert_eq!(merged.change_interval, Some(150));
        assert!(merged.changed);
    }

    #[test]
    fn server_session_continues_until_idle_timeout() {
        let current = "server-session".to_string();
        let active = derive_session(Some(current.clone()), 1_000, 1_000 + SESSION_IDLE_MILLIS);
        assert_eq!(active.id, current);
        assert!(!active.started_new);

        let idle = derive_session(
            Some("server-session".into()),
            1_000,
            1_001 + SESSION_IDLE_MILLIS,
        );
        assert_ne!(idle.id, "server-session");
        assert!(idle.started_new);
    }

    #[test]
    fn heartbeat_advances_activity_without_incrementing_telemetry_items() {
        let row = DeviceRow {
            last_seen_at: 100,
            last_event_at: Some(100),
            last_metric_at: None,
            last_log_at: None,
            last_error_at: None,
            last_session_id: None,
            last_session_at: None,
            last_app_version: None,
            last_app_version_at: None,
            last_launcher_version: None,
            last_launcher_version_at: None,
            last_os: None,
            last_os_at: None,
            last_system_language: None,
            last_system_language_at: None,
            last_architecture: None,
            last_architecture_at: None,
            event_items: 4,
            metric_items: 0,
            log_items: 0,
            error_items: 0,
            session_changes: 0,
            app_version_changes: 0,
            launcher_version_changes: 0,
            os_changes: 0,
            risk_score: 0,
            last_anomaly: None,
            last_anomaly_at: None,
        };
        let observation = DeviceObservation {
            kind: DeviceTelemetryKind::Heartbeat,
            received_at: 200,
            telemetry_at: 200,
            item_count: 0,
            session_id: None,
            app_version: None,
            launcher_version: None,
            os: None,
            system_language: None,
            architecture: None,
        };
        let counters = update_counters(&row, &observation);
        assert_eq!(counters.event_items, 4);
        assert_eq!(counters.metric_items, 0);
        assert_eq!(counters.last_event_at, Some(100));
    }
}
