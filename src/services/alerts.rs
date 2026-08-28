use std::time::Duration;

use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr,
    sea_query::{Alias, Expr, ExprTrait, Func, Query},
};
use tracing::{error, info, warn};

use crate::{
    database::alert_repo::{
        self, AlertDeliveryRecord, AlertRuleRecord, NewRule, NotificationChannelRecord, UpdateRule,
    },
    domain::alert::{AlertExpression, AlertSource},
    error::AppError,
    services::authentication::AuthenticatedUser,
    state::InstalledState,
};

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

// ----------------------------------------------------
// Channels Management
// ----------------------------------------------------

pub async fn list_channels(
    installed: &InstalledState,
    user: &AuthenticatedUser,
) -> Result<Vec<NotificationChannelRecord>, AppError> {
    user.require("alerts.read", None)?;
    Ok(alert_repo::list_channels(&installed.database).await?)
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
        return Err(AppError::Validation("channel name and kind are required".into()));
    }
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
        return Err(AppError::Validation("channel name and kind are required".into()));
    }
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

    dispatch_to_channel(&channel, &test_payload).await.map_err(|err| {
        AppError::Validation(format!("Failed to send test notification: {}", err))
    })?;

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

// ----------------------------------------------------
// Alert Evaluator Engine
// ----------------------------------------------------

pub async fn evaluate_all_rules(database: &DatabaseConnection) -> Result<usize, DbErr> {
    let rules = alert_repo::list_rules(database, None).await?;
    let channels = alert_repo::list_channels(database).await?;
    let active_channels: Vec<_> = channels.into_iter().filter(|c| c.enabled).collect();

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

        let window_ms = (expression.window_minutes as i64) * 60_000;
        let window_start = now - window_ms;

        let (condition_met, current_value) = match evaluate_rule_condition(database, &rule.application_id, &expression, window_start, now).await {
            Ok(res) => res,
            Err(err) => {
                warn!(rule_id = %rule.id, error = %err, "error evaluating alert condition");
                continue;
            }
        };

        let is_currently_firing = rule.last_state == "firing";
        let cooldown_ms = (rule.cooldown_seconds as i64) * 1_000;
        let cooldown_passed = rule.last_evaluated_at.is_none_or(|last| now - last >= cooldown_ms);

        if condition_met {
            if !is_currently_firing || cooldown_passed {
                info!(
                    rule_id = %rule.id,
                    rule_name = %rule.name,
                    val = %current_value,
                    threshold = %expression.threshold,
                    "Alert FIRING"
                );
                let _ = alert_repo::update_rule_state(database, &rule.id, "firing", now).await;

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

                for channel in &active_channels {
                    let res = dispatch_to_channel(channel, &alert_payload).await;
                    let (status, err_msg) = match res {
                        Ok(_) => ("delivered", None),
                        Err(e) => ("failed", Some(e)),
                    };
                    let _ = alert_repo::record_delivery(database, &rule.id, &channel.id, status, 1, err_msg.as_deref()).await;
                }
            }
        } else if is_currently_firing {
            info!(rule_id = %rule.id, rule_name = %rule.name, "Alert RESOLVED");
            let _ = alert_repo::update_rule_state(database, &rule.id, "healthy", now).await;

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

            for channel in &active_channels {
                let res = dispatch_to_channel(channel, &resolved_payload).await;
                let (status, err_msg) = match res {
                    Ok(_) => ("delivered", None),
                    Err(e) => ("failed", Some(e)),
                };
                let _ = alert_repo::record_delivery(database, &rule.id, &channel.id, status, 1, err_msg.as_deref()).await;
            }
        }
    }

    Ok(evaluated)
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
            let mut q = Query::select();
            q.expr(Func::count(Expr::col(Alias::new("id"))))
                .from(Alias::new("events"))
                .and_where(Expr::col(Alias::new("application_id")).eq(app_id))
                .and_where(Expr::col(Alias::new("timestamp")).gte(window_start))
                .and_where(Expr::col(Alias::new("timestamp")).lte(window_end));
            for f in &expression.filters {
                q.and_where(Expr::col(Alias::new(&f.field)).eq(&f.value));
            }
            let row = database.query_one(&q).await?;
            row.and_then(|r| r.try_get::<i64>("", "count").ok()).unwrap_or(0) as f64
        }
        AlertSource::LogCount => {
            let mut q = Query::select();
            q.expr(Func::count(Expr::col(Alias::new("id"))))
                .from(Alias::new("logs"))
                .and_where(Expr::col(Alias::new("application_id")).eq(app_id))
                .and_where(Expr::col(Alias::new("timestamp")).gte(window_start))
                .and_where(Expr::col(Alias::new("timestamp")).lte(window_end));
            for f in &expression.filters {
                q.and_where(Expr::col(Alias::new(&f.field)).eq(&f.value));
            }
            let row = database.query_one(&q).await?;
            row.and_then(|r| r.try_get::<i64>("", "count").ok()).unwrap_or(0) as f64
        }
        AlertSource::MetricAverage => {
            let mut q = Query::select();
            q.expr(Func::avg(Expr::col(Alias::new("value"))))
                .from(Alias::new("metric_points"))
                .and_where(Expr::col(Alias::new("application_id")).eq(app_id))
                .and_where(Expr::col(Alias::new("timestamp")).gte(window_start))
                .and_where(Expr::col(Alias::new("timestamp")).lte(window_end));
            for f in &expression.filters {
                q.and_where(Expr::col(Alias::new(&f.field)).eq(&f.value));
            }
            let row = database.query_one(&q).await?;
            row.and_then(|r| r.try_get::<f64>("", "avg").ok()).unwrap_or(0.0)
        }
        AlertSource::MetricSum => {
            let mut q = Query::select();
            q.expr(Func::sum(Expr::col(Alias::new("value"))))
                .from(Alias::new("metric_points"))
                .and_where(Expr::col(Alias::new("application_id")).eq(app_id))
                .and_where(Expr::col(Alias::new("timestamp")).gte(window_start))
                .and_where(Expr::col(Alias::new("timestamp")).lte(window_end));
            for f in &expression.filters {
                q.and_where(Expr::col(Alias::new(&f.field)).eq(&f.value));
            }
            let row = database.query_one(&q).await?;
            row.and_then(|r| r.try_get::<f64>("", "sum").ok()).unwrap_or(0.0)
        }
        AlertSource::MissingData => {
            let mut q = Query::select();
            q.expr(Func::count(Expr::col(Alias::new("id"))))
                .from(Alias::new("events"))
                .and_where(Expr::col(Alias::new("application_id")).eq(app_id))
                .and_where(Expr::col(Alias::new("timestamp")).gte(window_start))
                .and_where(Expr::col(Alias::new("timestamp")).lte(window_end));
            let row = database.query_one(&q).await?;
            let count = row.and_then(|r| r.try_get::<i64>("", "count").ok()).unwrap_or(0);
            return Ok((count == 0, count as f64));
        }
        AlertSource::ChangeRate => {
            let curr_q = Query::select()
                .expr(Func::count(Expr::col(Alias::new("id"))))
                .from(Alias::new("events"))
                .and_where(Expr::col(Alias::new("application_id")).eq(app_id))
                .and_where(Expr::col(Alias::new("timestamp")).gte(window_start))
                .and_where(Expr::col(Alias::new("timestamp")).lte(window_end))
                .to_owned();
            let prev_start = window_start - (window_end - window_start);
            let prev_q = Query::select()
                .expr(Func::count(Expr::col(Alias::new("id"))))
                .from(Alias::new("events"))
                .and_where(Expr::col(Alias::new("application_id")).eq(app_id))
                .and_where(Expr::col(Alias::new("timestamp")).gte(prev_start))
                .and_where(Expr::col(Alias::new("timestamp")).lt(window_start))
                .to_owned();

            let curr_cnt = database
                .query_one(&curr_q)
                .await?
                .and_then(|r| r.try_get::<i64>("", "count").ok())
                .unwrap_or(0) as f64;
            let prev_cnt = database
                .query_one(&prev_q)
                .await?
                .and_then(|r| r.try_get::<i64>("", "count").ok())
                .unwrap_or(0) as f64;

            let rate = if prev_cnt > 0.0 {
                ((curr_cnt - prev_cnt) / prev_cnt) * 100.0
            } else {
                0.0
            };
            return Ok((expression.operator.evaluate(rate, expression.threshold), rate));
        }
    };

    let met = expression.operator.evaluate(value, expression.threshold);
    Ok((met, value))
}

async fn dispatch_to_channel(
    channel: &NotificationChannelRecord,
    payload: &serde_json::Value,
) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

    let msg = payload.get("message").and_then(|v| v.as_str()).unwrap_or("Sonde Alert");
    let status = payload.get("status").and_then(|v| v.as_str()).unwrap_or("alert");

    match channel.kind.as_str() {
        "webhook" => {
            let url = channel
                .config
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or("Missing 'url' in channel config")?;

            let mut req = client.post(url).header("content-type", "application/json");
            if let Some(headers) = channel.config.get("headers").and_then(|v| v.as_object()) {
                for (k, v) in headers {
                    if let Some(val_str) = v.as_str() {
                        req = req.header(k.as_str(), val_str);
                    }
                }
            }

            let resp = req
                .json(payload)
                .send()
                .await
                .map_err(|e| format!("HTTP request failed: {}", e))?;

            if resp.status().is_success() {
                Ok(())
            } else {
                let s = resp.status();
                let body = resp.text().await.unwrap_or_default();
                Err(format!("HTTP status {}: {}", s, body))
            }
        }
        "slack" => {
            let url = channel
                .config
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or("Missing 'url' in channel config")?;

            let body = serde_json::json!({
                "text": format!("🚨 *[Sonde Telemetry]* ({})\n{}", status.to_uppercase(), msg)
            });

            let resp = client.post(url).json(&body).send().await.map_err(|e| format!("Slack request failed: {}", e))?;
            if resp.status().is_success() { Ok(()) } else { Err(format!("Slack HTTP {}", resp.status())) }
        }
        "discord" => {
            let url = channel
                .config
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or("Missing 'url' in channel config")?;

            let body = serde_json::json!({
                "content": format!("🚨 **[Sonde Telemetry]** ({})\n{}", status.to_uppercase(), msg)
            });

            let resp = client.post(url).json(&body).send().await.map_err(|e| format!("Discord request failed: {}", e))?;
            if resp.status().is_success() { Ok(()) } else { Err(format!("Discord HTTP {}", resp.status())) }
        }
        "feishu" => {
            let url = channel
                .config
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or("Missing 'url' in channel config")?;

            let body = serde_json::json!({
                "msg_type": "text",
                "content": {
                    "text": format!("🚨 [Sonde 遥测告警通知]\n状态: {}\n信息: {}\n时间: {}", status.to_uppercase(), msg, chrono::Utc::now().to_rfc3339())
                }
            });

            let resp = client.post(url).json(&body).send().await.map_err(|e| format!("Feishu request failed: {}", e))?;
            if resp.status().is_success() { Ok(()) } else { Err(format!("Feishu HTTP {}", resp.status())) }
        }
        "dingtalk" => {
            let url = channel
                .config
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or("Missing 'url' in channel config")?;

            let body = serde_json::json!({
                "msgtype": "text",
                "text": {
                    "content": format!("🚨 [Sonde 遥测告警]\n状态: {}\n{}\n时间: {}", status.to_uppercase(), msg, chrono::Utc::now().to_rfc3339())
                }
            });

            let resp = client.post(url).json(&body).send().await.map_err(|e| format!("DingTalk request failed: {}", e))?;
            if resp.status().is_success() { Ok(()) } else { Err(format!("DingTalk HTTP {}", resp.status())) }
        }
        "wecom" => {
            let url = channel
                .config
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or("Missing 'url' in channel config")?;

            let body = serde_json::json!({
                "msgtype": "text",
                "text": {
                    "content": format!("🚨 [Sonde 告警事件]\n状态: {}\n{}\n时间: {}", status.to_uppercase(), msg, chrono::Utc::now().to_rfc3339())
                }
            });

            let resp = client.post(url).json(&body).send().await.map_err(|e| format!("WeCom request failed: {}", e))?;
            if resp.status().is_success() { Ok(()) } else { Err(format!("WeCom HTTP {}", resp.status())) }
        }
        "telegram" => {
            let bot_token = channel.config.get("botToken").and_then(|v| v.as_str()).ok_or("Missing 'botToken'")?;
            let chat_id = channel.config.get("chatId").and_then(|v| v.as_str()).ok_or("Missing 'chatId'")?;
            let url = format!("https://api.telegram.org/bot{}/sendMessage", bot_token);

            let body = serde_json::json!({
                "chat_id": chat_id,
                "text": format!("🚨 *[Sonde Telemetry]* ({})\n{}", status.to_uppercase(), msg),
                "parse_mode": "Markdown"
            });

            let resp = client.post(&url).json(&body).send().await.map_err(|e| format!("Telegram request failed: {}", e))?;
            if resp.status().is_success() { Ok(()) } else { Err(format!("Telegram HTTP {}", resp.status())) }
        }
        "email" => {
            if let Some(url) = channel.config.get("url").and_then(|v| v.as_str()) {
                let resp = client
                    .post(url)
                    .header("content-type", "application/json")
                    .json(payload)
                    .send()
                    .await
                    .map_err(|e| format!("Email webhook failed: {}", e))?;
                if resp.status().is_success() {
                    Ok(())
                } else {
                    Err(format!("Email webhook HTTP {}", resp.status()))
                }
            } else {
                Ok(())
            }
        }
        _ => Err(format!("Unsupported notification channel kind '{}'", channel.kind)),
    }
}

pub fn spawn_alert_evaluator_worker(database: DatabaseConnection) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            interval.tick().await;
            if let Err(err) = evaluate_all_rules(&database).await {
                error!(error = %err, "alert evaluator worker encountered an error");
            }
        }
    });
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
