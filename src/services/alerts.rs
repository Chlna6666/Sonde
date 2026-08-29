use std::{net::Ipv4Addr, sync::OnceLock, time::Duration};

use reqwest::redirect::Policy;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, DbErr,
    sea_query::{Alias, Expr, ExprTrait, Func, Query},
};
use tracing::{info, warn};
use url::{Host, Url};

use crate::{
    database::{
        alert_delivery_repo,
        alert_repo::{
            self, AlertDeliveryRecord, AlertRuleRecord, NewRule, NotificationChannelRecord,
            UpdateRule,
        },
    },
    domain::alert::{AlertExpression, AlertSource},
    error::AppError,
    services::authentication::AuthenticatedUser,
    state::InstalledState,
};

const DELIVERY_MAX_ATTEMPTS: i32 = 3;
const DELIVERY_FIRST_RETRY_MILLIS: i64 = 1_000;
const DELIVERY_SECOND_RETRY_MILLIS: i64 = 5_000;
const DELIVERY_ERROR_MAX_CHARS: usize = 2_048;

pub async fn list(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    application_id: Option<&str>,
) -> Result<Vec<AlertRuleRecord>, AppError> {
    user.require("alerts.read", application_id)?;
    Ok(alert_repo::list_rules(&installed.database, application_id).await?)
}

pub async fn create(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    application_id: &str,
    name: &str,
    expression: &AlertExpression,
    cooldown_seconds: i32,
) -> Result<String, AppError> {
    user.require("alerts.manage", Some(application_id))?;
    expression
        .validate()
        .map_err(|message| AppError::Validation(message.into()))?;
    if name.trim().is_empty() || cooldown_seconds < 0 {
        return Err(AppError::Validation("invalid rule name or cooldown".into()));
    }
    let query = serde_json::to_value(expression).map_err(|_| AppError::Internal)?;
    let id = alert_repo::create_rule(
        &installed.database,
        NewRule {
            application_id,
            name,
            source_kind: source_name(expression),
            query: &query,
            window_minutes: i32::from(expression.window_minutes),
            cooldown_seconds,
        },
    )
    .await?;

    crate::database::app_repo::audit(
        &installed.database,
        Some(&user.id),
        "alert_rule.created",
        "alert_rule",
        Some(&id),
    )
    .await?;

    Ok(id)
}

pub async fn update(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    id: &str,
    name: &str,
    expression: &AlertExpression,
    cooldown_seconds: i32,
    enabled: bool,
) -> Result<(), AppError> {
    let rule = alert_repo::get_rule(&installed.database, id)
        .await?
        .ok_or(AppError::NotFound)?;
    user.require("alerts.manage", Some(&rule.application_id))?;

    expression
        .validate()
        .map_err(|message| AppError::Validation(message.into()))?;
    if name.trim().is_empty() || cooldown_seconds < 0 {
        return Err(AppError::Validation("invalid rule name or cooldown".into()));
    }
    let query = serde_json::to_value(expression).map_err(|_| AppError::Internal)?;
    alert_repo::update_rule(
        &installed.database,
        id,
        UpdateRule {
            name,
            enabled,
            source_kind: source_name(expression),
            query: &query,
            window_minutes: i32::from(expression.window_minutes),
            cooldown_seconds,
        },
    )
    .await?;

    crate::database::app_repo::audit(
        &installed.database,
        Some(&user.id),
        "alert_rule.updated",
        "alert_rule",
        Some(id),
    )
    .await?;

    Ok(())
}

pub async fn delete(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    id: &str,
) -> Result<(), AppError> {
    let rule = alert_repo::get_rule(&installed.database, id)
        .await?
        .ok_or(AppError::NotFound)?;
    user.require("alerts.manage", Some(&rule.application_id))?;

    let deleted = alert_repo::delete_rule(&installed.database, id).await?;
    if !deleted {
        return Err(AppError::NotFound);
    }

    crate::database::app_repo::audit(
        &installed.database,
        Some(&user.id),
        "alert_rule.deleted",
        "alert_rule",
        Some(id),
    )
    .await?;

    Ok(())
}

pub async fn list_channels(
    installed: &InstalledState,
    user: &AuthenticatedUser,
) -> Result<Vec<NotificationChannelRecord>, AppError> {
    user.require("alerts.read", None)?;
    let mut channels = alert_repo::list_channels(&installed.database).await?;
    for channel in &mut channels {
        redact_channel_config(&mut channel.config);
    }
    Ok(channels)
}

pub async fn create_channel(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    name: &str,
    kind: &str,
    config: &serde_json::Value,
    enabled: bool,
) -> Result<String, AppError> {
    user.require("alerts.manage", None)?;
    if name.trim().is_empty() || kind.trim().is_empty() {
        return Err(AppError::Validation(
            "channel name and kind are required".into(),
        ));
    }
    validate_channel_config(kind, config).map_err(AppError::Validation)?;
    let id = alert_repo::create_channel(&installed.database, name, kind, config, enabled).await?;

    crate::database::app_repo::audit(
        &installed.database,
        Some(&user.id),
        "notification_channel.created",
        "notification_channel",
        Some(&id),
    )
    .await?;

    Ok(id)
}

pub async fn update_channel(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    id: &str,
    name: &str,
    kind: &str,
    config: &serde_json::Value,
    enabled: bool,
) -> Result<(), AppError> {
    user.require("alerts.manage", None)?;
    if name.trim().is_empty() || kind.trim().is_empty() {
        return Err(AppError::Validation(
            "channel name and kind are required".into(),
        ));
    }
    validate_channel_config(kind, config).map_err(AppError::Validation)?;
    alert_repo::update_channel(&installed.database, id, name, kind, config, enabled).await?;

    crate::database::app_repo::audit(
        &installed.database,
        Some(&user.id),
        "notification_channel.updated",
        "notification_channel",
        Some(id),
    )
    .await?;

    Ok(())
}

pub async fn delete_channel(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    id: &str,
) -> Result<(), AppError> {
    user.require("alerts.manage", None)?;
    let deleted = alert_repo::delete_channel(&installed.database, id).await?;
    if !deleted {
        return Err(AppError::NotFound);
    }

    crate::database::app_repo::audit(
        &installed.database,
        Some(&user.id),
        "notification_channel.deleted",
        "notification_channel",
        Some(id),
    )
    .await?;

    Ok(())
}

pub async fn test_channel(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    id: &str,
) -> Result<(), AppError> {
    user.require("alerts.manage", None)?;
    let channel = alert_repo::get_channel(&installed.database, id)
        .await?
        .ok_or(AppError::NotFound)?;

    let test_payload = serde_json::json!({
        "event": "test_notification",
        "system": "Sonde Telemetry",
        "channel": channel.name,
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "message": format!("Test notification from Sonde for channel '{}'", channel.name)
    });

    dispatch_to_channel(&channel, &test_payload)
        .await
        .map_err(|err| AppError::Validation(format!("Failed to send test notification: {err}")))?;

    Ok(())
}

pub async fn list_deliveries(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    limit: Option<u64>,
) -> Result<Vec<AlertDeliveryRecord>, AppError> {
    user.require("alerts.read", None)?;
    let limit = limit.unwrap_or(50).clamp(1, 200);
    Ok(alert_repo::list_deliveries(&installed.database, limit).await?)
}

pub async fn evaluate_all_rules(database: &DatabaseConnection) -> Result<usize, DbErr> {
    let rules = alert_repo::list_rules(database, None).await?;
    let active_channel_ids = alert_repo::list_channels(database)
        .await?
        .into_iter()
        .filter(|channel| channel.enabled)
        .map(|channel| channel.id)
        .collect::<Vec<_>>();

    let now = chrono::Utc::now().timestamp_millis();
    let mut evaluated = 0;

    for rule in rules {
        if !rule.enabled {
            continue;
        }
        evaluated += 1;

        let Ok(expression) = serde_json::from_value::<AlertExpression>(rule.query.clone()) else {
            warn!(rule_id = %rule.id, "failed to parse alert expression");
            continue;
        };

        let window_ms = i64::from(expression.window_minutes) * 60_000;
        let window_start = now - window_ms;
        let (condition_met, current_value) =
            match evaluate_rule_condition(database, &rule.application_id, &expression, window_start, now)
                .await
            {
                Ok(result) => result,
                Err(err) => {
                    warn!(rule_id = %rule.id, error = %err, "error evaluating alert condition");
                    continue;
                }
            };

        let is_currently_firing = rule.last_state == "firing";
        let pending_hits = parse_pending_hits(&rule.last_state);
        let cooldown_ms = i64::from(rule.cooldown_seconds) * 1_000;
        let cooldown_passed = rule
            .last_evaluated_at
            .is_none_or(|last| now - last >= cooldown_ms);

        if condition_met {
            if !is_currently_firing {
                let next_hits = pending_hits.saturating_add(1);
                if next_hits < expression.consecutive_hits {
                    let pending_state = format!("pending:{next_hits}");
                    alert_repo::update_rule_state(database, &rule.id, &pending_state, now).await?;
                    continue;
                }
            }

            if !is_currently_firing || cooldown_passed {
                info!(
                    rule_id = %rule.id,
                    rule_name = %rule.name,
                    val = %current_value,
                    threshold = %expression.threshold,
                    consecutive_hits = expression.consecutive_hits,
                    "Alert FIRING"
                );
                let alert_payload = serde_json::json!({
                    "status": "firing",
                    "ruleId": rule.id,
                    "ruleName": rule.name,
                    "applicationId": rule.application_id,
                    "sourceKind": rule.source_kind,
                    "currentValue": current_value,
                    "threshold": expression.threshold,
                    "windowMinutes": expression.window_minutes,
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                    "message": format!("Alert rule '{}' is FIRING. Current value: {:.2} (Threshold: {:.2})", rule.name, current_value, expression.threshold)
                });
                alert_delivery_repo::persist_transition(
                    database,
                    &rule.id,
                    "firing",
                    now,
                    &active_channel_ids,
                    &alert_payload,
                )
                .await?;
            }
        } else if is_currently_firing {
            info!(rule_id = %rule.id, rule_name = %rule.name, "Alert RESOLVED");
            let resolved_payload = serde_json::json!({
                "status": "resolved",
                "ruleId": rule.id,
                "ruleName": rule.name,
                "applicationId": rule.application_id,
                "sourceKind": rule.source_kind,
                "currentValue": current_value,
                "threshold": expression.threshold,
                "windowMinutes": expression.window_minutes,
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "message": format!("Alert rule '{}' has RECOVERED and is now healthy.", rule.name)
            });
            alert_delivery_repo::persist_transition(
                database,
                &rule.id,
                "healthy",
                now,
                &active_channel_ids,
                &resolved_payload,
            )
            .await?;
        } else if pending_hits > 0 {
            alert_repo::update_rule_state(database, &rule.id, "healthy", now).await?;
        }
    }

    Ok(evaluated)
}

pub async fn process_due_deliveries(
    database: &DatabaseConnection,
    limit: u64,
) -> Result<usize, DbErr> {
    let deliveries = alert_delivery_repo::list_due(
        database,
        chrono::Utc::now().timestamp_millis(),
        limit,
    )
    .await?;
    let mut processed = 0_usize;

    for delivery in deliveries {
        let payload = match serde_json::from_str::<serde_json::Value>(&delivery.payload_json) {
            Ok(payload) => payload,
            Err(error) => {
                let message = truncate_delivery_error(&format!(
                    "invalid persisted alert payload: {error}"
                ));
                if alert_delivery_repo::mark_failed(
                    database,
                    &delivery.id,
                    delivery.attempts,
                    false,
                    &message,
                )
                .await?
                {
                    processed = processed.saturating_add(1);
                }
                continue;
            }
        };

        let Some(channel) = alert_repo::get_channel(database, &delivery.channel_id).await? else {
            if alert_delivery_repo::mark_failed(
                database,
                &delivery.id,
                delivery.attempts,
                false,
                "notification channel no longer exists",
            )
            .await?
            {
                processed = processed.saturating_add(1);
            }
            continue;
        };
        if !channel.enabled {
            if alert_delivery_repo::mark_failed(
                database,
                &delivery.id,
                delivery.attempts,
                false,
                "notification channel is disabled",
            )
            .await?
            {
                processed = processed.saturating_add(1);
            }
            continue;
        }

        match dispatch_to_channel(&channel, &payload).await {
            Ok(()) => {
                if alert_delivery_repo::mark_delivered(
                    database,
                    &delivery.id,
                    delivery.attempts,
                )
                .await?
                {
                    processed = processed.saturating_add(1);
                } else {
                    warn!(delivery_id = %delivery.id, "alert delivery state changed before success acknowledgement");
                }
            }
            Err(error) => {
                let message = truncate_delivery_error(&error);
                let attempt_number = delivery.attempts.saturating_add(1);
                let updated = if attempt_number >= DELIVERY_MAX_ATTEMPTS {
                    alert_delivery_repo::mark_failed(
                        database,
                        &delivery.id,
                        delivery.attempts,
                        true,
                        &message,
                    )
                    .await?
                } else {
                    let delay = if attempt_number <= 1 {
                        DELIVERY_FIRST_RETRY_MILLIS
                    } else {
                        DELIVERY_SECOND_RETRY_MILLIS
                    };
                    alert_delivery_repo::reschedule(
                        database,
                        &delivery.id,
                        delivery.attempts,
                        &message,
                        chrono::Utc::now().timestamp_millis().saturating_add(delay),
                    )
                    .await?
                };
                if updated {
                    processed = processed.saturating_add(1);
                } else {
                    warn!(delivery_id = %delivery.id, "alert delivery state changed before failure acknowledgement");
                }
            }
        }
        tokio::task::yield_now().await;
    }

    Ok(processed)
}

fn truncate_delivery_error(error: &str) -> String {
    error.chars().take(DELIVERY_ERROR_MAX_CHARS).collect()
}

fn parse_pending_hits(state: &str) -> u16 {
    state
        .strip_prefix("pending:")
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(0)
}

#[derive(Clone, Copy)]
enum MetricAggregate {
    Average,
    Sum,
}

async fn metric_aggregate_value(
    database: &DatabaseConnection,
    app_id: &str,
    expression: &AlertExpression,
    window_start: i64,
    window_end: i64,
    aggregate: MetricAggregate,
) -> Result<f64, DbErr> {
    let count_cast = match database.get_database_backend() {
        DbBackend::Postgres => "CAST(histogram_count AS DOUBLE PRECISION)",
        DbBackend::MySql => "CAST(histogram_count AS DOUBLE)",
        DbBackend::Sqlite => "CAST(histogram_count AS REAL)",
        _ => "CAST(histogram_count AS DOUBLE PRECISION)",
    };
    let contribution = "CASE WHEN metric_type = 'histogram' AND histogram_count IS NOT NULL THEN histogram_sum ELSE value END";
    let missing_sum = "CASE WHEN metric_type = 'histogram' AND histogram_count IS NOT NULL AND histogram_sum IS NULL THEN 1 ELSE NULL END";
    let weight = format!(
        "CASE WHEN metric_type = 'histogram' AND histogram_count IS NOT NULL AND histogram_sum IS NOT NULL THEN {count_cast} WHEN metric_type <> 'histogram' OR histogram_count IS NULL THEN 1.0 ELSE NULL END"
    );

    let mut query = Query::select();
    query
        .expr_as(Expr::cust(format!("SUM({contribution})")), Alias::new("total"))
        .expr_as(
            Func::count(Expr::cust(missing_sum)),
            Alias::new("missing_sums"),
        )
        .from(Alias::new("metric_points"))
        .and_where(Expr::col(Alias::new("application_id")).eq(app_id))
        .and_where(Expr::col(Alias::new("timestamp")).gte(window_start))
        .and_where(Expr::col(Alias::new("timestamp")).lte(window_end));
    if matches!(aggregate, MetricAggregate::Average) {
        query.expr_as(Expr::cust(format!("SUM({weight})")), Alias::new("weight"));
    }
    apply_alert_filters(&mut query, &expression.filters);

    let Some(row) = database.query_one(&query).await? else {
        return Ok(0.0);
    };
    let missing_sums = row.try_get::<i64>("", "missing_sums").unwrap_or(0);
    if missing_sums > 0 {
        return Err(DbErr::Custom(
            "histogram metric aggregate is not computable because sum is missing".into(),
        ));
    }
    let total = row.try_get::<Option<f64>>("", "total")?.unwrap_or(0.0);
    if matches!(aggregate, MetricAggregate::Sum) {
        return Ok(total);
    }
    let weight = row.try_get::<Option<f64>>("", "weight")?.unwrap_or(0.0);
    Ok(if weight > 0.0 { total / weight } else { 0.0 })
}

async fn evaluate_rule_condition(
    database: &DatabaseConnection,
    app_id: &str,
    expression: &AlertExpression,
    window_start: i64,
    window_end: i64,
) -> Result<(bool, f64), DbErr> {
    let value = match &expression.source {
        AlertSource::EventCount => {
            let mut query = Query::select();
            query
                .expr(Func::count(Expr::col(Alias::new("id"))))
                .from(Alias::new("events"))
                .and_where(Expr::col(Alias::new("application_id")).eq(app_id))
                .and_where(Expr::col(Alias::new("timestamp")).gte(window_start))
                .and_where(Expr::col(Alias::new("timestamp")).lte(window_end));
            apply_alert_filters(&mut query, &expression.filters);
            let row = database.query_one(&query).await?;
            row.and_then(|row| row.try_get::<i64>("", "count").ok())
                .unwrap_or(0) as f64
        }
        AlertSource::LogCount => {
            let mut query = Query::select();
            query
                .expr(Func::count(Expr::col(Alias::new("id"))))
                .from(Alias::new("logs"))
                .and_where(Expr::col(Alias::new("application_id")).eq(app_id))
                .and_where(Expr::col(Alias::new("timestamp")).gte(window_start))
                .and_where(Expr::col(Alias::new("timestamp")).lte(window_end));
            apply_alert_filters(&mut query, &expression.filters);
            let row = database.query_one(&query).await?;
            row.and_then(|row| row.try_get::<i64>("", "count").ok())
                .unwrap_or(0) as f64
        }
        AlertSource::MetricAverage => {
            metric_aggregate_value(
                database,
                app_id,
                expression,
                window_start,
                window_end,
                MetricAggregate::Average,
            )
            .await?
        }
        AlertSource::MetricSum => {
            metric_aggregate_value(
                database,
                app_id,
                expression,
                window_start,
                window_end,
                MetricAggregate::Sum,
            )
            .await?
        }
        AlertSource::MissingData => {
            let query = Query::select()
                .expr(Func::count(Expr::col(Alias::new("id"))))
                .from(Alias::new("events"))
                .and_where(Expr::col(Alias::new("application_id")).eq(app_id))
                .and_where(Expr::col(Alias::new("timestamp")).gte(window_start))
                .and_where(Expr::col(Alias::new("timestamp")).lte(window_end))
                .to_owned();
            let row = database.query_one(&query).await?;
            let count = row
                .and_then(|row| row.try_get::<i64>("", "count").ok())
                .unwrap_or(0);
            return Ok((count == 0, count as f64));
        }
        AlertSource::ChangeRate => {
            let curr_query = Query::select()
                .expr(Func::count(Expr::col(Alias::new("id"))))
                .from(Alias::new("events"))
                .and_where(Expr::col(Alias::new("application_id")).eq(app_id))
                .and_where(Expr::col(Alias::new("timestamp")).gte(window_start))
                .and_where(Expr::col(Alias::new("timestamp")).lte(window_end))
                .to_owned();
            let prev_start = window_start - (window_end - window_start);
            let prev_query = Query::select()
                .expr(Func::count(Expr::col(Alias::new("id"))))
                .from(Alias::new("events"))
                .and_where(Expr::col(Alias::new("application_id")).eq(app_id))
                .and_where(Expr::col(Alias::new("timestamp")).gte(prev_start))
                .and_where(Expr::col(Alias::new("timestamp")).lt(window_start))
                .to_owned();

            let current_count = database
                .query_one(&curr_query)
                .await?
                .and_then(|row| row.try_get::<i64>("", "count").ok())
                .unwrap_or(0) as f64;
            let previous_count = database
                .query_one(&prev_query)
                .await?
                .and_then(|row| row.try_get::<i64>("", "count").ok())
                .unwrap_or(0) as f64;

            let rate = if previous_count > 0.0 {
                ((current_count - previous_count) / previous_count) * 100.0
            } else {
                0.0
            };
            return Ok((
                expression.operator.evaluate(rate, expression.threshold),
                rate,
            ));
        }
    };

    Ok((
        expression.operator.evaluate(value, expression.threshold),
        value,
    ))
}

fn apply_alert_filters(
    query: &mut sea_orm::sea_query::SelectStatement,
    filters: &[crate::domain::alert::AlertFilter],
) {
    for filter in filters {
        query.and_where(Expr::col(Alias::new(&filter.field)).eq(&filter.value));
    }
}

async fn dispatch_to_channel(
    channel: &NotificationChannelRecord,
    payload: &serde_json::Value,
) -> Result<(), String> {
    let client = alert_http_client()?;
    let message = payload
        .get("message")
        .and_then(|value| value.as_str())
        .unwrap_or("Sonde Alert");
    let status = payload
        .get("status")
        .and_then(|value| value.as_str())
        .unwrap_or("alert");

    match channel.kind.as_str() {
        "webhook" => {
            let url = channel_url(&channel.config)?;
            let mut request = client.post(url).header("content-type", "application/json");
            if let Some(headers) = channel.config.get("headers").and_then(|value| value.as_object()) {
                for (key, value) in headers {
                    if let Some(value) = value.as_str() {
                        request = request.header(key.as_str(), value);
                    }
                }
            }
            let response = request
                .json(payload)
                .send()
                .await
                .map_err(|err| format!("HTTP request failed: {err}"))?;
            if response.status().is_success() {
                Ok(())
            } else {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                Err(format!("HTTP status {status}: {}", truncate_error_body(&body)))
            }
        }
        "slack" => {
            let url = channel_url(&channel.config)?;
            let body = serde_json::json!({
                "text": format!("🚨 *[Sonde Telemetry]* ({})\n{}", status.to_uppercase(), message)
            });
            send_json(&client, url, &body, "Slack").await
        }
        "discord" => {
            let url = channel_url(&channel.config)?;
            let body = serde_json::json!({
                "content": format!("🚨 **[Sonde Telemetry]** ({})\n{}", status.to_uppercase(), message)
            });
            send_json(&client, url, &body, "Discord").await
        }
        "feishu" => {
            let url = channel_url(&channel.config)?;
            let body = serde_json::json!({
                "msg_type": "text",
                "content": {
                    "text": format!("🚨 [Sonde 遥测告警通知]\n状态: {}\n信息: {}\n时间: {}", status.to_uppercase(), message, chrono::Utc::now().to_rfc3339())
                }
            });
            send_json(&client, url, &body, "Feishu").await
        }
        "dingtalk" => {
            let url = channel_url(&channel.config)?;
            let body = serde_json::json!({
                "msgtype": "text",
                "text": {
                    "content": format!("🚨 [Sonde 遥测告警]\n状态: {}\n{}\n时间: {}", status.to_uppercase(), message, chrono::Utc::now().to_rfc3339())
                }
            });
            send_json(&client, url, &body, "DingTalk").await
        }
        "wecom" => {
            let url = channel_url(&channel.config)?;
            let body = serde_json::json!({
                "msgtype": "text",
                "text": {
                    "content": format!("🚨 [Sonde 告警事件]\n状态: {}\n{}\n时间: {}", status.to_uppercase(), message, chrono::Utc::now().to_rfc3339())
                }
            });
            send_json(&client, url, &body, "WeCom").await
        }
        "telegram" => {
            let bot_token = channel
                .config
                .get("botToken")
                .and_then(|value| value.as_str())
                .filter(|value| !value.trim().is_empty())
                .ok_or("Missing 'botToken' in channel config")?;
            let chat_id = channel
                .config
                .get("chatId")
                .and_then(|value| value.as_str())
                .filter(|value| !value.trim().is_empty())
                .ok_or("Missing 'chatId' in channel config")?;
            let url = format!("https://api.telegram.org/bot{bot_token}/sendMessage");
            let body = serde_json::json!({
                "chat_id": chat_id,
                "text": format!("🚨 *[Sonde Telemetry]* ({})\n{}", status.to_uppercase(), message),
                "parse_mode": "Markdown"
            });
            send_json(&client, &url, &body, "Telegram").await
        }
        "email" => {
            let url = channel_url(&channel.config)?;
            send_json(&client, url, payload, "Email webhook").await
        }
        _ => Err(format!(
            "Unsupported notification channel kind '{}'",
            channel.kind
        )),
    }
}

async fn send_json(
    client: &reqwest::Client,
    url: &str,
    payload: &serde_json::Value,
    channel_name: &str,
) -> Result<(), String> {
    let response = client
        .post(url)
        .json(payload)
        .send()
        .await
        .map_err(|err| format!("{channel_name} request failed: {err}"))?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!("{channel_name} HTTP {}", response.status()))
    }
}

fn alert_http_client() -> Result<reqwest::Client, String> {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    if let Some(client) = CLIENT.get() {
        return Ok(client.clone());
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(Policy::none())
        .build()
        .map_err(|err| format!("Failed to build HTTP client: {err}"))?;
    let _ = CLIENT.set(client.clone());
    Ok(CLIENT.get().cloned().unwrap_or(client))
}

fn validate_channel_config(kind: &str, config: &serde_json::Value) -> Result<(), String> {
    match kind {
        "telegram" => {
            for key in ["botToken", "chatId"] {
                let value = config
                    .get(key)
                    .and_then(|value| value.as_str())
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| format!("{key} is required for telegram channels"))?;
                if value.len() > 512 {
                    return Err(format!("{key} is too long"));
                }
            }
            Ok(())
        }
        "webhook" | "slack" | "discord" | "feishu" | "dingtalk" | "wecom" | "email" => {
            let _ = channel_url(config)?;
            if kind == "webhook" {
                validate_custom_headers(config)?;
            }
            Ok(())
        }
        _ => Err(format!("unsupported notification channel kind '{kind}'")),
    }
}

fn validate_custom_headers(config: &serde_json::Value) -> Result<(), String> {
    let Some(headers) = config.get("headers") else {
        return Ok(());
    };
    let Some(headers) = headers.as_object() else {
        return Err("webhook headers must be an object".into());
    };
    if headers.len() > 32 {
        return Err("at most 32 custom webhook headers are allowed".into());
    }
    for (key, value) in headers {
        let Some(value) = value.as_str() else {
            return Err("webhook header values must be strings".into());
        };
        if key.is_empty() || key.len() > 128 || value.len() > 4_096 {
            return Err("webhook header name or value exceeds the allowed size".into());
        }
    }
    Ok(())
}

fn channel_url(config: &serde_json::Value) -> Result<&str, String> {
    let url = config
        .get("url")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .ok_or("Missing 'url' in channel config")?;
    validate_outbound_url(url)?;
    Ok(url)
}

fn validate_outbound_url(raw: &str) -> Result<(), String> {
    if raw.len() > 2_048 {
        return Err("notification URL is too long".into());
    }
    let parsed = Url::parse(raw).map_err(|_| "notification URL is invalid".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("notification URL must use http or https".into());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("notification URL must not contain userinfo credentials".into());
    }

    match parsed.host().ok_or("notification URL must contain a host")? {
        Host::Domain(domain) => {
            let domain = domain.trim_end_matches('.').to_ascii_lowercase();
            if domain == "localhost"
                || domain.ends_with(".localhost")
                || domain.ends_with(".local")
                || domain.ends_with(".internal")
                || domain == "metadata.google.internal"
            {
                return Err("notification URL must not target a local host".into());
            }
        }
        Host::Ipv4(address) => {
            if !is_public_ipv4(address) {
                return Err("notification URL must target a public IPv4 address".into());
            }
        }
        Host::Ipv6(address) => {
            if address.is_loopback()
                || address.is_unspecified()
                || address.is_unique_local()
                || address.is_unicast_link_local()
                || address.is_multicast()
            {
                return Err("notification URL must target a public IPv6 address".into());
            }
        }
    }
    Ok(())
}

fn is_public_ipv4(address: Ipv4Addr) -> bool {
    let [a, b, _, _] = address.octets();
    !(a == 0
        || a == 10
        || a == 127
        || a >= 224
        || (a == 100 && (64..=127).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 168)
        || (a == 198 && (18..=19).contains(&b)))
}

fn redact_channel_config(config: &mut serde_json::Value) {
    let Some(object) = config.as_object_mut() else {
        return;
    };
    for (key, value) in object {
        let normalized = key.to_ascii_lowercase();
        if normalized == "url"
            || normalized == "headers"
            || normalized.contains("token")
            || normalized.contains("secret")
            || normalized.contains("password")
            || normalized.contains("authorization")
            || normalized.contains("apikey")
            || normalized.contains("api_key")
        {
            *value = serde_json::Value::String("***redacted***".into());
        }
    }
}

fn truncate_error_body(body: &str) -> &str {
    let end = body
        .char_indices()
        .nth(1_024)
        .map_or(body.len(), |(index, _)| index);
    &body[..end]
}

fn source_name(expression: &AlertExpression) -> &'static str {
    match &expression.source {
        AlertSource::EventCount => "event_count",
        AlertSource::MetricAverage => "metric_average",
        AlertSource::MetricSum => "metric_sum",
        AlertSource::LogCount => "log_count",
        AlertSource::MissingData => "missing_data",
        AlertSource::ChangeRate => "change_rate",
    }
}

#[cfg(test)]
mod tests {
    use super::{is_public_ipv4, parse_pending_hits, truncate_delivery_error, validate_outbound_url};
    use std::net::Ipv4Addr;

    #[test]
    fn pending_alert_state_is_parsed() {
        assert_eq!(parse_pending_hits("pending:3"), 3);
        assert_eq!(parse_pending_hits("healthy"), 0);
    }

    #[test]
    fn private_notification_targets_are_rejected() {
        assert!(!is_public_ipv4(Ipv4Addr::new(127, 0, 0, 1)));
        assert!(!is_public_ipv4(Ipv4Addr::new(10, 0, 0, 1)));
        assert!(validate_outbound_url("http://127.0.0.1/hook").is_err());
        assert!(validate_outbound_url("http://localhost/hook").is_err());
        assert!(validate_outbound_url("https://example.com/hook").is_ok());
    }

    #[test]
    fn delivery_errors_are_bounded() {
        let error = "x".repeat(3_000);
        assert_eq!(truncate_delivery_error(&error).chars().count(), 2_048);
    }
}
