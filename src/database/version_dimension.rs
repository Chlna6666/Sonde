use std::collections::{BTreeMap, HashMap};

use super::dimension_rollup::DimensionDayCount;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionBucket {
    pub bucket: String,
    pub total: u64,
    pub versions: Vec<(String, u64)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionSeriesData {
    pub version: String,
    pub total: u64,
    pub points: Vec<(String, u64)>,
}

pub fn supports_daily_projection(bucket_expr: &str) -> bool {
    !bucket_expr.contains("HH24:00") && !bucket_expr.contains("%H:00")
}

pub fn timeline(points: &[DimensionDayCount], bucket_expr: &str) -> Vec<VersionBucket> {
    let mut buckets = BTreeMap::<String, BTreeMap<String, u64>>::new();
    for point in points {
        let bucket = projected_bucket(&point.day, bucket_expr);
        let count = buckets
            .entry(bucket)
            .or_default()
            .entry(point.value.clone())
            .or_insert(0);
        *count = count.saturating_add(point.count);
    }

    buckets
        .into_iter()
        .map(|(bucket, versions)| {
            let total = versions
                .values()
                .fold(0_u64, |sum, count| sum.saturating_add(*count));
            let mut versions = versions.into_iter().collect::<Vec<_>>();
            versions.sort_by(|left, right| {
                right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0))
            });
            VersionBucket {
                bucket,
                total,
                versions,
            }
        })
        .collect()
}

pub fn top_series(points: &[DimensionDayCount], bucket_expr: &str) -> Vec<VersionSeriesData> {
    let mut buckets = BTreeMap::<String, HashMap<String, u64>>::new();
    let mut totals = HashMap::<String, u64>::new();

    for point in points {
        let bucket = projected_bucket(&point.day, bucket_expr);
        let bucket_count = buckets
            .entry(bucket)
            .or_default()
            .entry(point.value.clone())
            .or_insert(0);
        *bucket_count = bucket_count.saturating_add(point.count);

        let total = totals.entry(point.value.clone()).or_insert(0);
        *total = total.saturating_add(point.count);
    }

    let all_buckets = buckets.keys().cloned().collect::<Vec<_>>();
    let mut top_versions = totals.into_iter().collect::<Vec<_>>();
    top_versions.sort_by(|left, right| {
        right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0))
    });
    top_versions.truncate(8);

    top_versions
        .into_iter()
        .map(|(version, total)| VersionSeriesData {
            points: all_buckets
                .iter()
                .map(|bucket| {
                    let count = buckets
                        .get(bucket)
                        .and_then(|versions| versions.get(&version))
                        .copied()
                        .unwrap_or(0);
                    (bucket.clone(), count)
                })
                .collect(),
            version,
            total,
        })
        .collect()
}

fn projected_bucket(day: &str, bucket_expr: &str) -> String {
    if bucket_expr == "day" {
        day.to_owned()
    } else {
        day.get(..7).unwrap_or(day).to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::{supports_daily_projection, timeline, top_series};
    use crate::database::dimension_rollup::DimensionDayCount;

    #[test]
    fn projects_daily_rows_to_month_and_top_series() {
        let points = vec![
            DimensionDayCount {
                day: "2026-07-31".into(),
                value: "1.0".into(),
                count: 2,
            },
            DimensionDayCount {
                day: "2026-08-01".into(),
                value: "1.0".into(),
                count: 3,
            },
            DimensionDayCount {
                day: "2026-08-01".into(),
                value: "2.0".into(),
                count: 5,
            },
        ];
        let timeline = timeline(&points, "strftime('%Y-%m', timestamp / 1000, 'unixepoch')");
        assert_eq!(timeline.len(), 2);
        assert_eq!(timeline[1].bucket, "2026-08");
        assert_eq!(timeline[1].total, 8);
        assert_eq!(timeline[1].versions[0], ("2.0".into(), 5));

        let series = top_series(&points, "strftime('%Y-%m', timestamp / 1000, 'unixepoch')");
        assert_eq!(series[0].version, "1.0");
        assert_eq!(series[0].total, 5);
        assert_eq!(series[0].points.len(), 2);
        assert_eq!(series[1].version, "2.0");
        assert_eq!(series[1].points[0].1, 0);
    }

    #[test]
    fn does_not_project_hourly_bucket() {
        assert!(!supports_daily_projection(
            "strftime('%Y-%m-%d %H:00', timestamp / 1000, 'unixepoch')"
        ));
        assert!(supports_daily_projection("day"));
    }
}
