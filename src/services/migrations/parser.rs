use std::collections::{BTreeMap, HashSet};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::error::AppError;

mod sql;

use sql::parse_statements;

#[derive(Clone, Debug)]
pub(super) struct LegacyEventRow {
    pub ts: i64,
    pub day: String,
    pub user_hash: String,
    pub app_version: Option<String>,
    pub launcher_version: Option<String>,
    pub os: Option<String>,
    pub key_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct D1Preview {
    pub source_hash: String,
    pub rows: usize,
    pub valid: usize,
    pub duplicates: usize,
    pub rejected: usize,
    pub first_day: Option<String>,
    pub last_day: Option<String>,
    pub app_versions: BTreeMap<String, usize>,
    pub launcher_versions: BTreeMap<String, usize>,
    pub operating_systems: BTreeMap<String, usize>,
}

#[derive(Debug)]
pub(crate) struct ParsedD1Export {
    pub preview: D1Preview,
    pub(super) rows: Vec<LegacyEventRow>,
}

pub(crate) fn parse_d1_export(sql: &str) -> Result<ParsedD1Export, AppError> {
    if sql.trim().is_empty() {
        return Err(AppError::Validation("the D1 export is empty".into()));
    }
    let source_hash = hex::encode(Sha256::digest(sql.as_bytes()));
    let (mut rows, rejected) = parse_statements(sql)?;
    let total_rows = rows.len() + rejected;
    rows.sort_by_key(|row| row.ts);
    let (rows, summary) = summarize(rows);
    Ok(ParsedD1Export {
        preview: D1Preview {
            source_hash,
            rows: total_rows,
            valid: rows.len(),
            duplicates: summary.duplicates,
            rejected,
            first_day: summary.first_day,
            last_day: summary.last_day,
            app_versions: summary.app_versions,
            launcher_versions: summary.launcher_versions,
            operating_systems: summary.operating_systems,
        },
        rows,
    })
}

struct PreviewSummary {
    duplicates: usize,
    first_day: Option<String>,
    last_day: Option<String>,
    app_versions: BTreeMap<String, usize>,
    launcher_versions: BTreeMap<String, usize>,
    operating_systems: BTreeMap<String, usize>,
}

fn summarize(rows: Vec<LegacyEventRow>) -> (Vec<LegacyEventRow>, PreviewSummary) {
    let mut seen = HashSet::new();
    let mut unique_rows = Vec::with_capacity(rows.len());
    let mut summary = PreviewSummary {
        duplicates: 0,
        first_day: None,
        last_day: None,
        app_versions: BTreeMap::new(),
        launcher_versions: BTreeMap::new(),
        operating_systems: BTreeMap::new(),
    };
    for row in rows {
        if !seen.insert((row.day.clone(), row.user_hash.clone())) {
            summary.duplicates += 1;
            continue;
        }
        summary.first_day = Some(
            summary
                .first_day
                .map_or_else(|| row.day.clone(), |day| day.min(row.day.clone())),
        );
        summary.last_day = Some(
            summary
                .last_day
                .map_or_else(|| row.day.clone(), |day| day.max(row.day.clone())),
        );
        count_value(&mut summary.app_versions, row.app_version.as_deref());
        count_value(
            &mut summary.launcher_versions,
            row.launcher_version.as_deref(),
        );
        count_value(&mut summary.operating_systems, row.os.as_deref());
        unique_rows.push(row);
    }
    (unique_rows, summary)
}

fn count_value(counts: &mut BTreeMap<String, usize>, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        *counts.entry(value.to_owned()).or_default() += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::parse_d1_export;

    #[test]
    fn parses_and_deduplicates_wrangler_export() -> Result<(), crate::error::AppError> {
        let sql = "CREATE TABLE events (ts INTEGER); INSERT INTO events (ts, day, user_hash, app_version, launcher_version, os, key_id) VALUES (1000, '2026-01-02', 'u1', '1.0', NULL, 'Windows', 'abc'), (900, '2026-01-02', 'u1', '0.9', NULL, 'Windows', 'abc');";
        let parsed = parse_d1_export(sql)?;
        assert_eq!(parsed.preview.rows, 2);
        assert_eq!(parsed.preview.valid, 1);
        assert_eq!(parsed.preview.duplicates, 1);
        assert_eq!(parsed.rows[0].ts, 900);
        Ok(())
    }

    #[test]
    fn rejects_non_whitelisted_statement() {
        assert!(parse_d1_export("DROP TABLE users;").is_err());
    }
}
