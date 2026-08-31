use std::path::Path;

use futures_util::Stream;

use crate::{
    database::{application_backup, applications, backup_archive, backup_validation, dimension_restore},
    error::AppError,
    services::{applications::ensure_app_access, authentication::AuthenticatedUser},
    state::InstalledState,
};

const APPLICATION_EXPORT_TYPE: &str = "sonde_application";
const MAX_HISTOGRAM_BOUNDS: usize = 256;

pub async fn export_application(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    application_id: &str,
) -> Result<application_backup::SingleAppExport, AppError> {
    ensure_app_access(&installed.database, user, application_id, false).await?;
    let Some(export_data) =
        application_backup::export_single_application(&installed.database, application_id).await?
    else {
        return Err(AppError::NotFound);
    };

    applications::audit(
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
    payload: application_backup::SingleAppExport,
) -> Result<String, AppError> {
    user.require("apps.manage", None)?;
    validate_application_backup_header(&payload.format_version, &payload.export_type)?;
    validate_application_backup_metrics(&payload)?;
    let new_app_id = application_backup::import_single_application(
        &installed.database,
        Some(&user.id),
        payload,
    )
    .await?;

    applications::audit(
        &installed.database,
        Some(&user.id),
        "application.imported",
        "application",
        Some(&new_app_id),
    )
    .await?;

    Ok(new_app_id)
}

pub async fn export_system_backup(
    installed: &InstalledState,
    user: &AuthenticatedUser,
) -> Result<
    impl Stream<Item = Result<Vec<u8>, backup_archive::BackupError>> + use<>,
    AppError,
> {
    require_system_backup_access(user)?;

    applications::audit(
        &installed.database,
        Some(&user.id),
        "system.backup_export_requested",
        "system",
        None,
    )
    .await?;

    Ok(backup_archive::export_full_system_stream(
        installed.database.clone(),
    ))
}

pub async fn restore_system_backup(
    installed: &InstalledState,
    user: &AuthenticatedUser,
    path: &Path,
) -> Result<u64, AppError> {
    require_system_backup_access(user)?;

    let restored = backup_validation::restore_full_system_exact_validated(
        &installed.database,
        path,
    )
    .await
    .map_err(map_backup_error)?;

    // Rollups are derived cache state. Readers fall back to authoritative raw telemetry until the
    // normal dirty-day workers rebuild every projection after an exact restore.
    dimension_restore::reset_after_full_restore(&installed.database).await?;

    // Exact restore clears sessions and may replace the initiating account, so audit as system.
    applications::audit(
        &installed.database,
        None,
        "system.backup_restored",
        "system",
        None,
    )
    .await?;

    Ok(restored)
}

fn validate_application_backup_header(version: &str, export_type: &str) -> Result<(), AppError> {
    if version != application_backup::FORMAT_VERSION {
        return Err(AppError::Validation(format!(
            "unsupported application backup format version {version}"
        )));
    }
    if export_type != APPLICATION_EXPORT_TYPE {
        return Err(AppError::Validation(format!(
            "unexpected application backup type {export_type}"
        )));
    }
    Ok(())
}

fn validate_application_backup_metrics(
    payload: &application_backup::SingleAppExport,
) -> Result<(), AppError> {
    for metric in &payload.telemetry.metric_points {
        validate_metric_backup(&metric.metric_type, metric.value, metric.histogram.as_ref())?;
    }
    Ok(())
}

fn validate_metric_backup(
    metric_type: &str,
    value: f64,
    histogram: Option<&application_backup::HistogramBackup>,
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
            let histogram = histogram.ok_or_else(|| {
                AppError::Validation("histogram metric is missing population data".into())
            })?;
            validate_histogram_backup(histogram)?;
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
    histogram: &application_backup::HistogramBackup,
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

fn map_backup_error(error: backup_archive::BackupError) -> AppError {
    match error {
        backup_archive::BackupError::Database(error) => AppError::from(error),
        backup_archive::BackupError::Invalid(message) => AppError::Validation(message),
        backup_archive::BackupError::Json(error) => {
            AppError::Validation(format!("invalid backup JSON: {error}"))
        }
        backup_archive::BackupError::Io(error) => {
            AppError::internal("restore backup file", error)
        }
    }
}
