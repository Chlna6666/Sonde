use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr, ExecResult,
    sea_query::{Alias, Expr, ExprTrait, Query, Value},
};

pub async fn insert(
    database: &impl ConnectionTrait,
    table: &str,
    columns: &[&str],
    values: Vec<Value>,
) -> Result<ExecResult, DbErr> {
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
    if rows.is_empty() {
        return Ok(0);
    }
    let mut query = Query::insert();
    query.into_table(Alias::new(table));
    query.columns(columns.iter().map(|column| Alias::new(*column)));
    for row in rows {
        query
            .values(row.into_iter().map(Expr::value))
            .map_err(|error| DbErr::Custom(error.to_string()))?;
    }
    Ok(database.execute(&query).await?.rows_affected())
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
