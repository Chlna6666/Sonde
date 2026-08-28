use std::collections::HashMap;

use crate::error::AppError;

use super::LegacyEventRow;

pub(super) fn parse_statements(sql: &str) -> Result<(Vec<LegacyEventRow>, usize), AppError> {
    let mut rows = Vec::new();
    let mut rejected = 0;
    for raw_statement in split_statements(sql)? {
        let statement = without_comments(&raw_statement);
        let canonical = statement
            .trim()
            .to_ascii_lowercase()
            .replace(['`', '"'], "");
        if is_allowed_metadata_statement(&canonical) {
            continue;
        }
        if canonical.starts_with("insert into events") {
            let parsed = parse_insert(&statement)?;
            rejected += parsed.1;
            rows.extend(parsed.0);
            continue;
        }
        return Err(AppError::Validation(
            "the export contains a statement outside the D1 events whitelist".into(),
        ));
    }
    Ok((rows, rejected))
}

fn is_allowed_metadata_statement(statement: &str) -> bool {
    statement.is_empty()
        || statement.starts_with("pragma ")
        || statement == "begin transaction"
        || statement == "commit"
        || statement.starts_with("create table events")
        || statement.starts_with("create table if not exists events")
        || (statement.starts_with("create index") && statement.contains(" on events"))
        || (statement.starts_with("create unique index") && statement.contains(" on events"))
        || statement.starts_with("delete from sqlite_sequence")
}

fn without_comments(statement: &str) -> String {
    statement
        .lines()
        .filter(|line| !line.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn split_statements(sql: &str) -> Result<Vec<String>, AppError> {
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut chars = sql.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '\'' {
            current.push(character);
            if quoted && chars.peek() == Some(&'\'') {
                current.push(chars.next().unwrap_or('\''));
            } else {
                quoted = !quoted;
            }
        } else if character == ';' && !quoted {
            statements.push(std::mem::take(&mut current));
        } else {
            current.push(character);
        }
    }
    if quoted {
        return Err(AppError::Validation("unterminated SQL string".into()));
    }
    if !current.trim().is_empty() {
        statements.push(current);
    }
    Ok(statements)
}

fn parse_insert(statement: &str) -> Result<(Vec<LegacyEventRow>, usize), AppError> {
    let open = statement.find('(').ok_or_else(invalid_insert)?;
    let close = statement[open..]
        .find(')')
        .map(|index| open + index)
        .ok_or_else(invalid_insert)?;
    let columns: Vec<String> = statement[open + 1..close]
        .split(',')
        .map(|column| column.trim().trim_matches(['`', '"']).to_ascii_lowercase())
        .collect();
    let values_at = statement[close + 1..]
        .to_ascii_lowercase()
        .find("values")
        .map(|index| close + 1 + index + "values".len())
        .ok_or_else(invalid_insert)?;
    let tuples = parse_tuples(&statement[values_at..])?;
    let mut rows = Vec::new();
    let mut rejected = 0;
    for values in tuples {
        if values.len() != columns.len() {
            rejected += 1;
            continue;
        }
        let fields: HashMap<&str, &Option<String>> = columns
            .iter()
            .zip(values.iter())
            .map(|(column, value)| (column.as_str(), value))
            .collect();
        match row_from_fields(&fields) {
            Some(row) => rows.push(row),
            None => rejected += 1,
        }
    }
    Ok((rows, rejected))
}

fn parse_tuples(input: &str) -> Result<Vec<Vec<Option<String>>>, AppError> {
    let mut tuples = Vec::new();
    let mut tuple = Vec::new();
    let mut token = String::new();
    let mut quoted = false;
    let mut inside = false;
    let mut chars = input.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '\'' => {
                if quoted && chars.peek() == Some(&'\'') {
                    token.push('\'');
                    chars.next();
                } else {
                    quoted = !quoted;
                }
            }
            '(' if !quoted && !inside => inside = true,
            ',' if !quoted && inside => tuple.push(sql_value(&token)),
            ')' if !quoted && inside => {
                tuple.push(sql_value(&token));
                tuples.push(std::mem::take(&mut tuple));
                token.clear();
                inside = false;
            }
            _ if inside => token.push(character),
            _ => {}
        }
        if character == ',' && !quoted && inside {
            token.clear();
        }
    }
    if quoted || inside {
        return Err(AppError::Validation("malformed INSERT values".into()));
    }
    Ok(tuples)
}

fn sql_value(token: &str) -> Option<String> {
    let value = token.trim();
    (!value.eq_ignore_ascii_case("null")).then(|| value.to_owned())
}

fn row_from_fields(fields: &HashMap<&str, &Option<String>>) -> Option<LegacyEventRow> {
    let required = |name: &str| fields.get(name).and_then(|value| value.as_deref());
    let optional = |name: &str| fields.get(name).and_then(|value| (*value).clone());
    Some(LegacyEventRow {
        ts: required("ts")?.parse().ok()?,
        day: required("day")?.to_owned(),
        user_hash: required("user_hash")?.to_owned(),
        app_version: optional("app_version"),
        launcher_version: optional("launcher_version"),
        os: optional("os"),
        key_id: optional("key_id"),
    })
}

fn invalid_insert() -> AppError {
    AppError::Validation("malformed events INSERT statement".into())
}
