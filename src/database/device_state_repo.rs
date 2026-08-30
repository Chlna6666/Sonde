use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr, TransactionTrait,
    sea_query::{Alias, Expr, ExprTrait, LockType, Query},
};

use super::{query::insert_batch_ignore_conflicts, telemetry_repo::TelemetryScope};

const DAY_MILLIS: i64 = 86_400_000;
const RAPID_SESSION_MILLIS: i64 = 30_000;
const RAPID_VERSION_MILLIS: i64 = 5 * 60_000;
const RAPID_OS_MILLIS: i64 = 60 * 60_000;

#[derive(Clone, Copy, Debug)]
pub enum DeviceTelemetryKind { Event, Metric, Log, Error }

#[derive(Clone, Debug)]
pub struct TimedDimension { pub value: String, pub timestamp: i64 }

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
}

#[derive(Debug)]
struct DeviceRow {
    last_seen_at: i64,
    last_event_at: Option<i64>, last_metric_at: Option<i64>, last_log_at: Option<i64>, last_error_at: Option<i64>,
    last_session_id: Option<String>, last_session_at: Option<i64>,
    last_app_version: Option<String>, last_app_version_at: Option<i64>,
    last_launcher_version: Option<String>, last_launcher_version_at: Option<i64>,
    last_os: Option<String>, last_os_at: Option<i64>,
    event_items: i64, metric_items: i64, log_items: i64, error_items: i64,
    session_changes: i64, app_version_changes: i64, launcher_version_changes: i64, os_changes: i64,
    risk_score: i32, last_anomaly: Option<String>, last_anomaly_at: Option<i64>,
}

#[derive(Debug)] struct DimensionMerge { value: Option<String>, timestamp: Option<i64>, changed: bool, change_interval: Option<i64> }

pub async fn observe(database: &DatabaseConnection, scope: &TelemetryScope, device_hash: &str, observation: &DeviceObservation) -> Result<(), DbErr> {
    let tx = database.begin().await?;
    ensure_device_row(&tx, scope, device_hash, observation.received_at).await?;
    let current = load_device_for_update(&tx, device_hash).await?.ok_or_else(|| DbErr::Custom("device profile row disappeared during update".into()))?;
    let session = merge_dimension(current.last_session_id, current.last_session_at, observation.session_id.as_ref());
    let app_version = merge_dimension(current.last_app_version, current.last_app_version_at, observation.app_version.as_ref());
    let launcher_version = merge_dimension(current.last_launcher_version, current.last_launcher_version_at, observation.launcher_version.as_ref());
    let os = merge_dimension(current.last_os, current.last_os_at, observation.os.as_ref());
    let mut flags = Vec::new();
    let elapsed_days = observation.received_at.saturating_sub(current.last_seen_at).div_euclid(DAY_MILLIS).clamp(0, i32::MAX as i64) as i32;
    let mut risk = current.risk_score.saturating_sub(elapsed_days).max(0);
    if observation.item_count >= 750 { add_risk(&mut risk, 5, &mut flags, "large_batch"); }
    let age = observation.received_at.saturating_sub(observation.telemetry_at);
    if age > DAY_MILLIS { add_risk(&mut risk, 3, &mut flags, "late_telemetry"); }
    if observation.telemetry_at > observation.received_at.saturating_add(60_000) { add_risk(&mut risk, 3, &mut flags, "clock_ahead"); }
    if session.changed && session.change_interval.is_some_and(|v| v <= RAPID_SESSION_MILLIS) { add_risk(&mut risk, 3, &mut flags, "rapid_session_change"); }
    if app_version.changed && app_version.change_interval.is_some_and(|v| v <= RAPID_VERSION_MILLIS) { add_risk(&mut risk, 8, &mut flags, "rapid_app_version_change"); }
    if launcher_version.changed && launcher_version.change_interval.is_some_and(|v| v <= RAPID_VERSION_MILLIS) { add_risk(&mut risk, 6, &mut flags, "rapid_launcher_version_change"); }
    if os.changed { add_risk(&mut risk, 15, &mut flags, "os_changed"); if os.change_interval.is_some_and(|v| v <= RAPID_OS_MILLIS) { add_risk(&mut risk, 10, &mut flags, "rapid_os_change"); } }
    let count = i64::try_from(observation.item_count).unwrap_or(i64::MAX);
    let mut event_items=current.event_items; let mut metric_items=current.metric_items; let mut log_items=current.log_items; let mut error_items=current.error_items;
    let mut last_event=current.last_event_at; let mut last_metric=current.last_metric_at; let mut last_log=current.last_log_at; let mut last_error=current.last_error_at;
    match observation.kind {
        DeviceTelemetryKind::Event => { event_items=event_items.saturating_add(count); last_event=max_timestamp(last_event, observation.received_at); }
        DeviceTelemetryKind::Metric => { metric_items=metric_items.saturating_add(count); last_metric=max_timestamp(last_metric, observation.received_at); }
        DeviceTelemetryKind::Log => { log_items=log_items.saturating_add(count); last_log=max_timestamp(last_log, observation.received_at); }
        DeviceTelemetryKind::Error => { error_items=error_items.saturating_add(count); last_error=max_timestamp(last_error, observation.received_at); }
    }
    let (last_anomaly,last_anomaly_at)=if flags.is_empty(){(current.last_anomaly,current.last_anomaly_at)}else{(Some(flags.join(",")),Some(observation.received_at))};
    let update=Query::update().table(Alias::new("telemetry_devices"))
        .value(Alias::new("last_seen_at"), current.last_seen_at.max(observation.received_at))
        .value(Alias::new("last_event_at"),last_event).value(Alias::new("last_metric_at"),last_metric).value(Alias::new("last_log_at"),last_log).value(Alias::new("last_error_at"),last_error)
        .value(Alias::new("last_session_id"),session.value).value(Alias::new("last_session_at"),session.timestamp)
        .value(Alias::new("last_app_version"),app_version.value).value(Alias::new("last_app_version_at"),app_version.timestamp)
        .value(Alias::new("last_launcher_version"),launcher_version.value).value(Alias::new("last_launcher_version_at"),launcher_version.timestamp)
        .value(Alias::new("last_os"),os.value).value(Alias::new("last_os_at"),os.timestamp)
        .value(Alias::new("event_items"),event_items).value(Alias::new("metric_items"),metric_items).value(Alias::new("log_items"),log_items).value(Alias::new("error_items"),error_items)
        .value(Alias::new("session_changes"),current.session_changes.saturating_add(if session.changed{1}else{0}))
        .value(Alias::new("app_version_changes"),current.app_version_changes.saturating_add(if app_version.changed{1}else{0}))
        .value(Alias::new("launcher_version_changes"),current.launcher_version_changes.saturating_add(if launcher_version.changed{1}else{0}))
        .value(Alias::new("os_changes"),current.os_changes.saturating_add(if os.changed{1}else{0}))
        .value(Alias::new("risk_score"),risk).value(Alias::new("last_anomaly"),last_anomaly).value(Alias::new("last_anomaly_at"),last_anomaly_at).value(Alias::new("updated_at"),observation.received_at)
        .and_where(Expr::col(Alias::new("id")).eq(device_hash)).to_owned();
    tx.execute(&update).await?; tx.commit().await
}

async fn ensure_device_row(db:&impl ConnectionTrait, scope:&TelemetryScope, id:&str, now:i64)->Result<(),DbErr>{
    insert_batch_ignore_conflicts(db,"telemetry_devices",&["id","application_id","environment_id","device_hash","last_seen_at","last_event_at","last_metric_at","last_log_at","last_error_at","last_session_id","last_session_at","last_app_version","last_app_version_at","last_launcher_version","last_launcher_version_at","last_os","last_os_at","event_items","metric_items","log_items","error_items","session_changes","app_version_changes","launcher_version_changes","os_changes","risk_score","last_anomaly","last_anomaly_at","updated_at"],vec![vec![id.to_owned().into(),scope.application_id.clone().into(),scope.environment_id.clone().into(),id.to_owned().into(),now.into(),Option::<i64>::None.into(),Option::<i64>::None.into(),Option::<i64>::None.into(),Option::<i64>::None.into(),Option::<String>::None.into(),Option::<i64>::None.into(),Option::<String>::None.into(),Option::<i64>::None.into(),Option::<String>::None.into(),Option::<i64>::None.into(),Option::<String>::None.into(),Option::<i64>::None.into(),0_i64.into(),0_i64.into(),0_i64.into(),0_i64.into(),0_i64.into(),0_i64.into(),0_i64.into(),0_i64.into(),0_i32.into(),Option::<String>::None.into(),Option::<i64>::None.into(),now.into()]],"id","id").await?; Ok(())
}

async fn load_device_for_update(db:&impl ConnectionTrait,id:&str)->Result<Option<DeviceRow>,DbErr>{
    let q=Query::select().columns(["last_seen_at","last_event_at","last_metric_at","last_log_at","last_error_at","last_session_id","last_session_at","last_app_version","last_app_version_at","last_launcher_version","last_launcher_version_at","last_os","last_os_at","event_items","metric_items","log_items","error_items","session_changes","app_version_changes","launcher_version_changes","os_changes","risk_score","last_anomaly","last_anomaly_at"].map(Alias::new)).from(Alias::new("telemetry_devices")).and_where(Expr::col(Alias::new("id")).eq(id)).lock(LockType::Update).limit(1).to_owned();
    db.query_one(&q).await?.map(|r|Ok(DeviceRow{last_seen_at:r.try_get("","last_seen_at")?,last_event_at:r.try_get("","last_event_at")?,last_metric_at:r.try_get("","last_metric_at")?,last_log_at:r.try_get("","last_log_at")?,last_error_at:r.try_get("","last_error_at")?,last_session_id:r.try_get("","last_session_id")?,last_session_at:r.try_get("","last_session_at")?,last_app_version:r.try_get("","last_app_version")?,last_app_version_at:r.try_get("","last_app_version_at")?,last_launcher_version:r.try_get("","last_launcher_version")?,last_launcher_version_at:r.try_get("","last_launcher_version_at")?,last_os:r.try_get("","last_os")?,last_os_at:r.try_get("","last_os_at")?,event_items:r.try_get("","event_items")?,metric_items:r.try_get("","metric_items")?,log_items:r.try_get("","log_items")?,error_items:r.try_get("","error_items")?,session_changes:r.try_get("","session_changes")?,app_version_changes:r.try_get("","app_version_changes")?,launcher_version_changes:r.try_get("","launcher_version_changes")?,os_changes:r.try_get("","os_changes")?,risk_score:r.try_get("","risk_score")?,last_anomaly:r.try_get("","last_anomaly")?,last_anomaly_at:r.try_get("","last_anomaly_at")?})).transpose()
}

fn merge_dimension(current:Option<String>,ts:Option<i64>,incoming:Option<&TimedDimension>)->DimensionMerge{let Some(i)=incoming else{return DimensionMerge{value:current,timestamp:ts,changed:false,change_interval:None}};if ts.is_some_and(|t|i.timestamp<t){return DimensionMerge{value:current,timestamp:ts,changed:false,change_interval:None}}let changed=current.as_deref().is_some_and(|v|v!=i.value.as_str());DimensionMerge{value:Some(i.value.clone()),timestamp:Some(i.timestamp),changed,change_interval:if changed{ts.map(|t|i.timestamp.saturating_sub(t).max(0))}else{None}}}
fn max_timestamp(current:Option<i64>,incoming:i64)->Option<i64>{Some(current.map_or(incoming,|v|v.max(incoming)))}
fn add_risk(score:&mut i32,delta:i32,flags:&mut Vec<&'static str>,flag:&'static str){*score=score.saturating_add(delta).min(100);flags.push(flag)}
