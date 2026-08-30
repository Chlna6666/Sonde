use std::path::Path;

use futures_util::Stream;

use crate::{
    database::{
        app_repo, backup_v2_repo, backup_v2_validation_repo, dimension_restore_repo,
        legacy_backup_repo,
    },
    error::AppError,
    services::{applications::ensure_app_access, authentication::AuthenticatedUser},
    state::InstalledState,
};

const LEGACY_JSON_FORMAT_VERSION: &str = "1.0";
const APPLICATION_EXPORT_TYPE: &str = "sonde_application";
const SYSTEM_BACKUP_TYPE: &str = "sonde_full_backup";
const MAX_HISTOGRAM_BOUNDS: usize = 256;

pub async fn export_application(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    application_id: &str,
) -> Result<legacy_backup_repo::SingleAppExport, AppError> {
    ensure_app_access(&installed.database, user, application_id, false).await?;
    let Some(export_data) =
        legacy_backup_repo::export_single_application(&installed.database, application_id).await?
    else {
        return Err(AppError::NotFound);
    };

    app_repo::audit(
        &installed.database,
        Some(&user.id),
        "application.exported",
        "application",
        Some(application_id),
    )
    .await?;

    Ok(export_data)
}

pub async fn import_application(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    payload: legacy_backup_repo::SingleAppExport,
) -> Result<String, AppError> {
    user.require("apps.manage", None)?;
    validate_backup_header(
        &payload.format_version,
        &payload.export_type,
        APPLICATION_EXPORT_TYPE,
    )?;
    validate_application_backup_metrics(&payload)?;
    let new_app_id = legacy_backup_repo::import_single_application(
        &installed.database,
        Some(&user.id),
        payload,
    )
    .await?;

    app_repo::audit(
        &installed.database,
        Some(&user.id),
        "application.imported",
        "application",
        Some(&new_app_id),
    )
    .await?;

    Ok(new_app_id)
}

pub async fn export_full_system(
    installed: &InstalledState,
    user: &AuthenticatedUser,
) -> Result<legacy_backup_repo::FullSystemBackup, AppError> {
    require_system_backup_access(user)?;

    let backup_data = legacy_backup_repo::export_full_system(&installed.database).await?;

    app_repo::audit(
        &installed.database,
        Some(&user.id),
        "system.backup_exported",
        "system",
        None,
    )
    .await?;

    Ok(backup_data)
}

pub async fn restore_full_system(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    payload: legacy_backup_repo::FullSystemBackup,
) -> Result<(), AppError> {
    require_system_backup_access(user)?;
    validate_backup_header(
        &payload.format_version,
        &payload.backup_type,
        SYSTEM_BACKUP_TYPE,
    )?;
    validate_system_backup_metrics(&payload)?;

    legacy_backup_repo::restore_full_system(&installed.database, payload).await?;
    dimension_restore_repo::reset_after_full_restore(&installed.database).await?;

    app_repo::audit(
        &installed.database,
        Some(&user.id),
        "system.backup_restored",
        "system",
        None,
    )
    .await?;

    Ok(())
}

pub async fn export_full_system_v2(
    installed: &InstalledState,
    user: &AuthenticatedUser,
) -> Result<impl Stream<Item = Result<Vec<u8>, backup_v2_repo::BackupV2Error>>, AppError> {
    require_system_backup_access(user)?;

    app_repo::audit(
        &installed.database,
        Some(&user.id),
        "system.backup_export_requested",
        "system",
        None,
    )
    .await?;

    Ok(backup_v2_repo::export_full_system_stream(
        installed.database.clone(),
    ))
}

pub async fn restore_full_system_v2(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    path: &Path,
) -> Result<u64, AppError> {
    require_system_backup_access(user)?;

    let restored = backup_v2_validation_repo::restore_full_system_exact_validated(
        &installed.database,
        path,
    )
    .await
    .map_err(map_backup_v2_error)?;

    // Telemetry rollups are derived cache state and are intentionally rebuilt from restored raw
    // telemetry. Readiness is invalidated atomically by exact restore, so concurrent readers use
    // authoritative raw data until the normal dirty-day workers have rebuilt all projections.
    dimension_restore_repo::reset_after_full_restore(&installed.database).await?;

    // Full restore intentionally clears auth_sessions and may replace the account that initiated the
    // request. Record the successful operation as a system actor instead of persisting a dangling id.
    app_repo::audit(
        &installed.database,
        None,
        "system.backup_v2_restored",
        "system",
        None,
    )
    .await?;

    Ok(restored)
}

fn validate_backup_header(
    version: &str,
    actual_type: &str,
    expected_type: &str,
) -> Result<(), AppError> {
    if !matches!(
        version,
        legacy_backup_repo::FORMAT_VERSION | LEGACY_JSON_FORMAT_VERSION
    ) {
        return Err(AppError::Validation(format!(
            "unsupported backup format version {version}"
        )));
    }
    if actual_type != expected_type {
        return Err(AppError::Validation(format!(
            "unexpected backup type {actual_type}"
        )));
    }
    Ok(())
}

fn validate_application_backup_metrics(
    payload: &legacy_backup_repo::SingleAppExport,
) -> Result<(), AppError> {
    for metric in &payload.telemetry.metric_points {
        validate_metric_backup(&metric.metric_type, metric.value, metric.histogram.as_ref())?;
    }
    Ok(())
}

fn validate_system_backup_metrics(
    payload: &legacy_backup_repo::FullSystemBackup,
) -> Result<(), AppError> {
    for metric in &payload.metric_points {
        validate_metric_backup(&metric.metric_type, metric.value, metric.histogram.as_ref())?;
    }
    Ok(())
}

fn validate_metric_backup(
    metric_type: &str,
    value: f64,
    histogram: Option<&legacy_backup_repo::HistogramBackup>,
) -> Result<(), AppError> {
    if !value.is_finite() {
        return Err(AppError::Validation("metric value must be finite".into()));
    }
    match metric_type {
        "gauge" | "counter" => {
            if histogram.is_some() {
                return Err(AppError::Validation(format!(
                    "{metric_type} metrics must not include histogram population"
                )));
            }
        }
        "histogram" => {
            // Legacy 1.0 histogram rows only had a scalar observation and remain valid without the
            // population object. New 1.1 rows are validated below.
            if let Some(histogram) = histogram {
                validate_histogram_backup(histogram)?;
            }
        }
        _ => {
            return Err(AppError::Validation(format!(
                "unsupported metric type {metric_type}"
            )));
        }
    }
    Ok(())
}

fn validate_histogram_backup(
    histogram: &legacy_backup_repo::HistogramBackup,
) -> Result<(), AppError> {
    if histogram.count > i64::MAX as u64 {
        return Err(AppError::Validation(
            "histogram count exceeds database range".into(),
        ));
    }
    if histogram.explicit_bounds.len() > MAX_HISTOGRAM_BOUNDS {
        return Err(AppError::Validation(
            "histogram supports at most 256 explicit bounds".into(),
        ));
    }
    for value in histogram
        .explicit_bounds
        .iter()
        .copied()
        .chain(histogram.sum)
        .chain(histogram.min)
        .chain(histogram.max)
    {
        if !value.is_finite() {
            return Err(AppError::Validation(
                "histogram contains a non-finite value".into(),
            ));
        }
    }
    if histogram
        .explicit_bounds
        .windows(2)
        .any(|window| window[0] >= window[1])
    {
        return Err(AppError::Validation(
            "histogram bounds must be strictly increasing".into(),
        ));
    }
    if histogram.bucket_counts.len() != histogram.explicit_bounds.len().saturating_add(1) {
        return Err(AppError::Validation(
            "histogram bucket count length must equal bounds length plus one".into(),
        ));
    }
    let bucket_total = histogram
        .bucket_counts
        .iter()
        .try_fold(0_u64, |total, count| total.checked_add(*count))
        .ok_or_else(|| AppError::Validation("histogram bucket counts overflow".into()))?;
    if bucket_total != histogram.count {
        return Err(AppError::Validation(
            "histogram bucket counts must sum to histogram count".into(),
        ));
    }
    if histogram
        .min
        .zip(histogram.max)
        .is_some_and(|(min, max)| min > max)
    {
        return Err(AppError::Validation(
            "histogram min must not exceed max".into(),
        ));
    }
    if histogram.count == 0 {
        if histogram.sum.is_some_and(|sum| sum != 0.0) {
            return Err(AppError::Validation(
                "empty histogram sum must be zero when provided".into(),
            ));
        }
        if histogram.min.is_some() || histogram.max.is_some() {
            return Err(AppError::Validation(
                "empty histogram must not include min or max".into(),
            ));
        }
    }
    Ok(())
}

fn require_system_backup_access(user: &AuthenticatedUser) -> Result<(), AppError> {
    if user
        .roles
        .iter()
        .any(|role| role == "Super Admin" || role == "Admin")
        || user.grants.iter().any(|grant| grant.allows("*", None))
    {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

fn map_backup_v2_error(error: backup_v2_repo::BackupV2Error) -> AppError {
    match error {
        backup_v2_repo::BackupV2Error::Database(error) => AppError::from(error),
        backup_v2_repo::BackupV2Error::Invalid(message) => AppError::Validation(message),
        backup_v2_repo::BackupV2Error::Json(error) => {
            AppError::Validation(format!("invalid backup JSON: {error}"))
        }
        backup_v2_repo::BackupV2Error::Io(error) => {
            AppError::internal("restore backup file", error)
        }
    }
}
