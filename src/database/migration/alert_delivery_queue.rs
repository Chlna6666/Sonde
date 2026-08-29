use sea_orm_migration::prelude::*;

use super::columns::create_index;

pub(super) struct AlertDeliveryQueue;

impl MigrationName for AlertDeliveryQueue {
    fn name(&self) -> &str {
        "m20260829_000021_alert_delivery_queue"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for AlertDeliveryQueue {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if !manager
            .has_column("alert_deliveries", "payload_json")
            .await?
        {
            manager
                .alter_table(
                    Table::alter()
                        .table(Alias::new("alert_deliveries"))
                        .add_column(
                            ColumnDef::new(Alias::new("payload_json"))
                                .text()
                                .null()
                                .to_owned(),
                        )
                        .to_owned(),
                )
                .await?;
        }

        create_index(
            manager,
            "idx_alert_deliveries_due",
            "alert_deliveries",
            &["status", "next_attempt_at", "created_at"],
            false,
        )
        .await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Queue payloads are additive and intentionally retained for portable SQLite downgrade
        // semantics. Older Sonde versions simply ignore the column and index.
        Ok(())
    }
}
