use std::{future::Future, sync::Arc, time::Duration};

use sea_orm::{DatabaseConnection, DbBackend, DbErr};
use tokio::sync::{Semaphore, mpsc, oneshot};

use crate::{
    database::telemetry::{self, TelemetryScope},
    domain::telemetry::{ErrorInput, EventInput, LogInput, MetricInput},
    error::AppError,
};

const QUEUE_CAPACITY: usize = 256;
const MAX_BATCH_REQUESTS: usize = 32;
const MAX_BATCH_ITEMS: usize = 4_000;
const BATCH_WINDOW: Duration = Duration::from_millis(5);
const REMOTE_DATABASE_WRITERS: usize = 4;

#[derive(Clone)]
pub struct IngestWriter {
    events: mpsc::Sender<WriteRequest<EventInput>>,
    metrics: mpsc::Sender<WriteRequest<MetricInput>>,
    logs: mpsc::Sender<WriteRequest<LogInput>>,
    errors: mpsc::Sender<WriteRequest<ErrorInput>>,
}

#[derive(Clone, Debug)]
enum WriteFailure {
    Database(String),
    Unavailable(&'static str),
}

struct WriteRequest<T> {
    scope: TelemetryScope,
    items: Vec<T>,
    response: oneshot::Sender<Result<(), WriteFailure>>,
}

struct WriteGroup<T> {
    scope: TelemetryScope,
    items: Vec<T>,
    responses: Vec<oneshot::Sender<Result<(), WriteFailure>>>,
}

impl IngestWriter {
    pub fn new(database: DatabaseConnection) -> Self {
        let writer_count = if database.get_database_backend() == DbBackend::Sqlite {
            1
        } else {
            REMOTE_DATABASE_WRITERS
        };
        let write_gate = Arc::new(Semaphore::new(writer_count));

        let (events, event_rx) = mpsc::channel(QUEUE_CAPACITY);
        spawn_lane(
            database.clone(),
            event_rx,
            write_gate.clone(),
            |database, scope, items| async move {
                telemetry::insert_events(&database, &scope, &items)
                    .await
                    .map(|_| ())
            },
        );

        let (metrics, metric_rx) = mpsc::channel(QUEUE_CAPACITY);
        spawn_lane(
            database.clone(),
            metric_rx,
            write_gate.clone(),
            |database, scope, items| async move {
                telemetry::insert_metrics(&database, &scope, &items)
                    .await
                    .map(|_| ())
            },
        );

        let (logs, log_rx) = mpsc::channel(QUEUE_CAPACITY);
        spawn_lane(
            database.clone(),
            log_rx,
            write_gate.clone(),
            |database, scope, items| async move {
                telemetry::insert_logs(&database, &scope, &items)
                    .await
                    .map(|_| ())
            },
        );

        let (errors, error_rx) = mpsc::channel(QUEUE_CAPACITY);
        spawn_lane(
            database,
            error_rx,
            write_gate,
            |database, scope, items| async move {
                telemetry::insert_errors(&database, &scope, &items)
                    .await
                    .map(|_| ())
            },
        );

        Self {
            events,
            metrics,
            logs,
            errors,
        }
    }

    pub async fn write_events(
        &self,
        scope: &TelemetryScope,
        items: Vec<EventInput>,
    ) -> Result<(), AppError> {
        enqueue(&self.events, scope, items).await
    }

    pub async fn write_metrics(
        &self,
        scope: &TelemetryScope,
        items: Vec<MetricInput>,
    ) -> Result<(), AppError> {
        enqueue(&self.metrics, scope, items).await
    }

    pub async fn write_logs(
        &self,
        scope: &TelemetryScope,
        items: Vec<LogInput>,
    ) -> Result<(), AppError> {
        enqueue(&self.logs, scope, items).await
    }

    pub async fn write_errors(
        &self,
        scope: &TelemetryScope,
        items: Vec<ErrorInput>,
    ) -> Result<(), AppError> {
        enqueue(&self.errors, scope, items).await
    }
}

async fn enqueue<T: Send + 'static>(
    sender: &mpsc::Sender<WriteRequest<T>>,
    scope: &TelemetryScope,
    items: Vec<T>,
) -> Result<(), AppError> {
    if items.is_empty() {
        return Ok(());
    }

    let (response, receiver) = oneshot::channel();
    let request = WriteRequest {
        scope: scope.clone(),
        items,
        response,
    };
    sender.try_send(request).map_err(|error| match error {
        mpsc::error::TrySendError::Full(_) => AppError::TooManyRequests,
        mpsc::error::TrySendError::Closed(_) => {
            AppError::unavailable("ingest writer queue", "writer task stopped")
        }
    })?;

    match receiver.await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(WriteFailure::Database(message))) => Err(AppError::from(DbErr::Custom(message))),
        Ok(Err(WriteFailure::Unavailable(message))) => {
            Err(AppError::unavailable("ingest writer", message))
        }
        Err(error) => Err(AppError::unavailable("ingest writer response", error)),
    }
}

fn spawn_lane<T, F, Fut>(
    database: DatabaseConnection,
    mut receiver: mpsc::Receiver<WriteRequest<T>>,
    write_gate: Arc<Semaphore>,
    insert: F,
) where
    T: Send + 'static,
    F: Fn(DatabaseConnection, TelemetryScope, Vec<T>) -> Fut + Send + Sync + Clone + 'static,
    Fut: Future<Output = Result<(), DbErr>> + Send + 'static,
{
    tokio::spawn(async move {
        while let Some(first) = receiver.recv().await {
            let mut pending = Vec::with_capacity(MAX_BATCH_REQUESTS);
            let mut pending_items = first.items.len();
            pending.push(first);

            let deadline = tokio::time::sleep(BATCH_WINDOW);
            tokio::pin!(deadline);
            while pending.len() < MAX_BATCH_REQUESTS && pending_items < MAX_BATCH_ITEMS {
                tokio::select! {
                    _ = &mut deadline => break,
                    request = receiver.recv() => {
                        let Some(request) = request else {
                            break;
                        };
                        pending_items = pending_items.saturating_add(request.items.len());
                        pending.push(request);
                    }
                }
            }

            let mut groups: Vec<WriteGroup<T>> = Vec::new();
            for mut request in pending {
                if let Some(group) = groups.iter_mut().find(|group| {
                    group.scope.application_id == request.scope.application_id
                        && group.scope.environment_id == request.scope.environment_id
                }) {
                    group.items.append(&mut request.items);
                    group.responses.push(request.response);
                } else {
                    groups.push(WriteGroup {
                        scope: request.scope,
                        items: std::mem::take(&mut request.items),
                        responses: vec![request.response],
                    });
                }
            }

            for group in groups {
                let permit = match write_gate.acquire().await {
                    Ok(permit) => permit,
                    Err(_) => {
                        for response in group.responses {
                            let _ = response.send(Err(WriteFailure::Unavailable(
                                "write concurrency gate closed",
                            )));
                        }
                        continue;
                    }
                };
                let result = insert(database.clone(), group.scope, group.items)
                    .await
                    .map_err(|error| WriteFailure::Database(error.to_string()));
                drop(permit);

                for response in group.responses {
                    let _ = response.send(result.clone());
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{MAX_BATCH_ITEMS, MAX_BATCH_REQUESTS, QUEUE_CAPACITY};

    #[test]
    fn writer_limits_are_bounded() {
        const { assert!(QUEUE_CAPACITY >= MAX_BATCH_REQUESTS) };
        const { assert!(MAX_BATCH_ITEMS >= MAX_BATCH_REQUESTS) };
    }
}
