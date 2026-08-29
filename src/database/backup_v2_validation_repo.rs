use std::path::Path;

use sea_orm::DatabaseConnection;
use tokio::{
    fs::File,
    io::{AsyncBufReadExt, BufReader},
};

use super::{
    backup_v2_repo::{self, BackupMetricPointV2, BackupV2Error, BackupV2Record},
    backup_v2_restore_repo,
};

const MAX_HISTOGRAM_BOUNDS: usize = 256;

/// Validate both the archive envelope (manifest/order/count/SHA-256) and record-level telemetry
/// invariants before a destructive exact restore starts.
pub async fn restore_full_system_exact_validated(
    database: &DatabaseConnection,
    path: &Path,
) -> Result<u64, BackupV2Error> {
    validate_backup_file_semantics(path).await?;
    backup_v2_restore_repo::restore_full_system_exact(database, path).await
}

pub async fn validate_backup_file_semantics(path: &Path) -> Result<(), BackupV2Error> {
    // The first pass proves record framing, manifest compatibility, record count and archive digest.
    // Only after the complete file is known to be structurally valid do we inspect record semantics.
    backup_v2_repo::validate_backup_file(path).await?;

    let file = File::open(path).await?;
    let mut reader = BufReader::new(file);
    let mut line = Vec::with_capacity(4096);
    loop {
        line.clear();
        let read = reader.read_until(b'\n', &mut line).await?;
        if read == 0 {
            break;
        }
        if line.len() > backup_v2_repo::MAX_RECORD_BYTES {
            return Err(BackupV2Error::Invalid(format!(
                "record exceeds {} bytes",
                backup_v2_repo::MAX_RECORD_BYTES
            )));
        }
        let record: BackupV2Record = serde_json::from_slice(record_payload(&line)?)?;
        if let BackupV2Record::MetricPoint(metric) = record {
            validate_metric(&metric)?;
        }
    }
    Ok(())
}

fn validate_metric(metric: &BackupMetricPointV2) -> Result<(), BackupV2Error> {
    if metric.name.is_empty() || metric.name.len() > 128 {
        return invalid("metric name must be 1..128 bytes");
    }
    if metric.unit.as_ref().is_some_and(|unit| unit.len() > 64) {
        return invalid("metric unit must be at most 64 bytes");
    }
    if !metric.value.is_finite() {
        return invalid("metric value must be finite");
    }
    if serde_json::from_str::<serde_json::Value>(&metric.attributes).is_err() {
        return invalid("metric attributes must contain valid JSON");
    }

    let has_histogram_payload = metric.histogram_count.is_some()
        || metric.histogram_sum.is_some()
        || metric.histogram_min.is_some()
        || metric.histogram_max.is_some()
        || metric.histogram_bounds.is_some()
        || metric.histogram_bucket_counts.is_some();

    match metric.metric_type.as_str() {
        "gauge" | "counter" => {
            if has_histogram_payload {
                return invalid("counter and gauge metrics must not include histogram data");
            }
        }
        "histogram" => {
            // v2.0 archives predate population columns and therefore legitimately carry only the
            // scalar compatibility observation. v2.1 population records are validated below.
            if !has_histogram_payload {
                return Ok(());
            }
            validate_histogram_population(metric)?;
        }
        _ => return invalid("unsupported metric type in backup"),
    }
    Ok(())
}

fn validate_histogram_population(metric: &BackupMetricPointV2) -> Result<(), BackupV2Error> {
    let count = metric
        .histogram_count
        .ok_or_else(|| BackupV2Error::Invalid("histogram count is missing".into()))?;
    let count = u64::try_from(count)
        .map_err(|_| BackupV2Error::Invalid("histogram count must not be negative".into()))?;
    let bounds_raw = metric
        .histogram_bounds
        .as_deref()
        .ok_or_else(|| BackupV2Error::Invalid("histogram bounds are missing".into()))?;
    let buckets_raw = metric
        .histogram_bucket_counts
        .as_deref()
        .ok_or_else(|| BackupV2Error::Invalid("histogram bucket counts are missing".into()))?;
    let bounds: Vec<f64> = serde_json::from_str(bounds_raw)
        .map_err(|_| BackupV2Error::Invalid("histogram bounds JSON is invalid".into()))?;
    let buckets: Vec<u64> = serde_json::from_str(buckets_raw)
        .map_err(|_| BackupV2Error::Invalid("histogram bucket counts JSON is invalid".into()))?;

    if bounds.len() > MAX_HISTOGRAM_BOUNDS {
        return invalid("histogram supports at most 256 explicit bounds");
    }
    if bounds.iter().any(|value| !value.is_finite())
        || metric.histogram_sum.is_some_and(|value| !value.is_finite())
        || metric.histogram_min.is_some_and(|value| !value.is_finite())
        || metric.histogram_max.is_some_and(|value| !value.is_finite())
    {
        return invalid("histogram contains a non-finite value");
    }
    if bounds.windows(2).any(|window| window[0] >= window[1]) {
        return invalid("histogram bounds must be strictly increasing");
    }
    if buckets.len() != bounds.len().saturating_add(1) {
        return invalid("histogram bucket count length must equal bounds length plus one");
    }
    let bucket_total = buckets
        .iter()
        .try_fold(0_u64, |total, value| total.checked_add(*value))
        .ok_or_else(|| BackupV2Error::Invalid("histogram bucket counts overflow".into()))?;
    if bucket_total != count {
        return invalid("histogram bucket counts must sum to histogram count");
    }
    if metric
        .histogram_min
        .zip(metric.histogram_max)
        .is_some_and(|(min, max)| min > max)
    {
        return invalid("histogram min must not exceed max");
    }
    if count == 0 {
        if metric.histogram_sum.is_some_and(|sum| sum != 0.0) {
            return invalid("empty histogram sum must be zero when provided");
        }
        if metric.histogram_min.is_some() || metric.histogram_max.is_some() {
            return invalid("empty histogram must not include min or max");
        }
    }
    Ok(())
}

fn record_payload(line: &[u8]) -> Result<&[u8], BackupV2Error> {
    let mut end = line.len();
    if end > 0 && line[end - 1] == b'\n' {
        end -= 1;
    }
    if end > 0 && line[end - 1] == b'\r' {
        end -= 1;
    }
    if end == 0 {
        return Err(BackupV2Error::Invalid("blank NDJSON record".into()));
    }
    Ok(&line[..end])
}

fn invalid<T>(message: &str) -> Result<T, BackupV2Error> {
    Err(BackupV2Error::Invalid(message.into()))
}
