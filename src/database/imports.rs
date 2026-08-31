use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr,
    sea_query::{Alias, Expr, ExprTrait, Query},
};
use serde::Serialize;

use super::query;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportRun {
    pub id: String,
    pub source_hash: String,
    pub application_id: String,
    pub environment_id: String,
    pub status: String,
    pub inserted: i64,
    pub deduped: i64,
    pub rejected: i64,
    pub created_at: i64,
}

pub async fn find_by_source_hash(
    database: &DatabaseConnection,
    source_hash: &str,
) -> Result<Option<ImportRun>, DbErr> {
    let select = Query::select()
        .columns(
            [
                "id",
                "source_hash",
                "application_id",
                "environment_id",
                "status",
                "inserted",
                "deduped",
                "rejected",
                "created_at",
            ]
            .map(Alias::new),
        )
        .from(Alias::new("import_runs"))
        .and_where(Expr::col(Alias::new("source_hash")).eq(source_hash))
        .limit(1)
        .to_owned();
    database
        .query_one(&select)
        .await?
        .map(|row| {
            Ok(ImportRun {
                id: row.try_get("", "id")?,
                source_hash: row.try_get("", "source_hash")?,
                application_id: row.try_get("", "application_id")?,
                environment_id: row.try_get("", "environment_id")?,
                status: row.try_get("", "status")?,
                inserted: row.try_get("", "inserted")?,
                deduped: row.try_get("", "deduped")?,
                rejected: row.try_get("", "rejected")?,
                created_at: row.try_get("", "created_at")?,
            })
        })
        .transpose()
}

pub async fn scope_exists(
    database: &DatabaseConnection,
    application_id: &str,
    environment_id: &str,
) -> Result<bool, DbErr> {
    let select = Query::select()
        .column(Alias::new("id"))
        .from(Alias::new("environments"))
        .and_where(Expr::col(Alias::new("id")).eq(environment_id))
        .and_where(Expr::col(Alias::new("application_id")).eq(application_id))
        .limit(1)
        .to_owned();
    Ok(database.query_one(&select).await?.is_some())
}

pub async fn list(database: &DatabaseConnection) -> Result<Vec<ImportRun>, DbErr> {
    let select = Query::select()
        .columns(
            [
                "id",
                "source_hash",
                "application_id",
                "environment_id",
                "status",
                "inserted",
                "deduped",
                "rejected",
                "created_at",
            ]
            .map(Alias::new),
        )
        .from(Alias::new("import_runs"))
        .order_by(Alias::new("created_at"), sea_orm::sea_query::Order::Desc)
        .limit(100)
        .to_owned();
    database
        .query_all(&select)
        .await?
        .into_iter()
        .map(|row| {
            Ok(ImportRun {
                id: row.try_get("", "id")?,
                source_hash: row.try_get("", "source_hash")?,
                application_id: row.try_get("", "application_id")?,
                environment_id: row.try_get("", "environment_id")?,
                status: row.try_get("", "status")?,
                inserted: row.try_get("", "inserted")?,
                deduped: row.try_get("", "deduped")?,
                rejected: row.try_get("", "rejected")?,
                created_at: row.try_get("", "created_at")?,
            })
        })
        .collect()
}

pub async fn create(database: &DatabaseConnection, run: &ImportRun) -> Result<(), DbErr> {
    query::insert(
        database,
        "import_runs",
        &[
            "id",
            "source_type",
            "source_hash",
            "application_id",
            "environment_id",
            "status",
            "inserted",
            "deduped",
            "rejected",
            "created_at",
        ],
        vec![
            run.id.clone().into(),
            "cloudflare_d1".into(),
            run.source_hash.clone().into(),
            run.application_id.clone().into(),
            run.environment_id.clone().into(),
            run.status.clone().into(),
            run.inserted.into(),
            run.deduped.into(),
            run.rejected.into(),
            run.created_at.into(),
        ],
    )
    .await?;
    Ok(())
}
