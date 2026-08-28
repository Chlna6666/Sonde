use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr, ExecResult,
    sea_query::{Alias, Expr, ExprTrait, OnConflict, Query, Value},
};

/// Keep a conservative cross-database bind budget. This remains below SQLite builds that still use
/// the historical 999-variable limit while also avoiding oversized prepared statements on MySQL
/// and PostgreSQL. Callers can therefore choose logical batches without knowing backend limits.
const SAFE_MAX_BIND_PARAMS: usize = 900;

pub async fn insert(
    database: &impl ConnectionTrait,
    table: &str,
    columns: &[&str],
    values: Vec<Value>,
) -> Result<ExecResult, DbErr> {
    if columns.is_empty() || values.len() != columns.len() {
        return Err(DbErr::Custom("insert column/value count mismatch".into()));
    }
    let mut query = Query::insert();
    query.into_table(Alias::new(table));
    query.columns(columns.iter().map(|column| Alias::new(*column)));
    query
        .values(values.into_iter().map(Expr::value))
        .map_err(|error| DbErr::Custom(error.to_string()))?;
    database.execute(&query).await
}

pub async fn insert_batch(
    database: &impl ConnectionTrait,
    table: &str,
    columns: &[&str],
    rows: Vec<Vec<Value>>,
) -> Result<u64, DbErr> {
    insert_batch_inner(database, table, columns, rows, None).await
}

/// Insert a batch while treating an existing unique idempotency value as a successful no-op.
///
/// `mysql_noop_column` is the column SeaQuery uses for its MySQL `ON DUPLICATE KEY UPDATE x=x`
/// compatibility form. PostgreSQL and SQLite use the actual conflict target.
pub async fn insert_batch_ignore_conflicts(
    database: &impl ConnectionTrait,
    table: &str,
    columns: &[&str],
    rows: Vec<Vec<Value>>,
    conflict_column: &str,
    mysql_noop_column: &str,
) -> Result<u64, DbErr> {
    insert_batch_inner(
        database,
        table,
        columns,
        rows,
        Some((conflict_column, mysql_noop_column)),
    )
    .await
}

async fn insert_batch_inner(
    database: &impl ConnectionTrait,
    table: &str,
    columns: &[&str],
    rows: Vec<Vec<Value>>,
    conflict: Option<(&str, &str)>,
) -> Result<u64, DbErr> {
    if rows.is_empty() {
        return Ok(0);
    }
    if columns.is_empty() {
        return Err(DbErr::Custom("batch insert requires at least one column".into()));
    }
    if rows.iter().any(|row| row.len() != columns.len()) {
        return Err(DbErr::Custom("batch insert column/value count mismatch".into()));
    }

    let rows_per_statement = (SAFE_MAX_BIND_PARAMS / columns.len()).max(1);
    let mut rows = rows.into_iter();
    let mut affected = 0_u64;

    loop {
        let mut query = Query::insert();
        query.into_table(Alias::new(table));
        query.columns(columns.iter().map(|column| Alias::new(*column)));

        let mut statement_rows = 0_usize;
        while statement_rows < rows_per_statement {
            let Some(row) = rows.next() else {
                break;
            };
            query
                .values(row.into_iter().map(Expr::value))
                .map_err(|error| DbErr::Custom(error.to_string()))?;
            statement_rows += 1;
        }

        if statement_rows == 0 {
            break;
        }

        if let Some((conflict_column, mysql_noop_column)) = conflict {
            query.on_conflict(
                OnConflict::column(Alias::new(conflict_column))
                    .do_nothing_on([Alias::new(mysql_noop_column)])
                    .to_owned(),
            );
        }
        affected = affected.saturating_add(database.execute(&query).await?.rows_affected());
    }

    Ok(affected)
}

pub async fn execute_delete(
    database: &DatabaseConnection,
    table: &str,
    column: &str,
    value: impl Into<Value>,
) -> Result<u64, DbErr> {
    let query = Query::delete()
        .from_table(Alias::new(table))
        .and_where(sea_orm::sea_query::Expr::col(Alias::new(column)).eq(value.into()))
        .to_owned();
    Ok(database.execute(&query).await?.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::SAFE_MAX_BIND_PARAMS;

    #[test]
    fn bind_budget_handles_wide_event_rows() {
        let event_columns = 14_usize;
        let rows = (SAFE_MAX_BIND_PARAMS / event_columns).max(1);
        assert!(rows * event_columns <= SAFE_MAX_BIND_PARAMS);
        assert!(rows >= 1);
    }
}
