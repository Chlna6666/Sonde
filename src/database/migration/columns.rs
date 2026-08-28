use sea_orm_migration::prelude::*;

pub(super) async fn create_table(
    manager: &SchemaManager<'_>,
    name: &str,
    columns: Vec<ColumnDef>,
) -> Result<(), DbErr> {
    let mut table = Table::create();
    table.table(Alias::new(name)).if_not_exists();
    for column in columns {
        table.col(column);
    }
    manager.create_table(table.to_owned()).await
}

pub(super) async fn create_index(
    manager: &SchemaManager<'_>,
    name: &str,
    table: &str,
    columns: &[&str],
    unique: bool,
) -> Result<(), DbErr> {
    let mut index = Index::create();
    index.name(name).table(Alias::new(table)).if_not_exists();
    if unique {
        index.unique();
    }
    for column in columns {
        index.col(Alias::new(*column));
    }
    manager.create_index(index.to_owned()).await
}

pub(super) fn string(name: &str) -> ColumnDef {
    ColumnDef::new(Alias::new(name))
        .string()
        .not_null()
        .to_owned()
}

pub(super) fn nullable_string(name: &str) -> ColumnDef {
    ColumnDef::new(Alias::new(name)).string().null().to_owned()
}

pub(super) fn text(name: &str) -> ColumnDef {
    ColumnDef::new(Alias::new(name))
        .text()
        .not_null()
        .to_owned()
}

pub(super) fn nullable_text(name: &str) -> ColumnDef {
    ColumnDef::new(Alias::new(name)).text().null().to_owned()
}

pub(super) fn bigint(name: &str) -> ColumnDef {
    ColumnDef::new(Alias::new(name))
        .big_integer()
        .not_null()
        .to_owned()
}

pub(super) fn nullable_bigint(name: &str) -> ColumnDef {
    ColumnDef::new(Alias::new(name))
        .big_integer()
        .null()
        .to_owned()
}

pub(super) fn integer(name: &str) -> ColumnDef {
    ColumnDef::new(Alias::new(name))
        .integer()
        .not_null()
        .to_owned()
}

pub(super) fn boolean(name: &str) -> ColumnDef {
    ColumnDef::new(Alias::new(name))
        .boolean()
        .not_null()
        .to_owned()
}

pub(super) fn double(name: &str) -> ColumnDef {
    ColumnDef::new(Alias::new(name))
        .double()
        .not_null()
        .to_owned()
}

pub(super) fn nullable_double(name: &str) -> ColumnDef {
    ColumnDef::new(Alias::new(name)).double().null().to_owned()
}
