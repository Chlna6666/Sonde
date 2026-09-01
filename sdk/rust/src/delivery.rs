use std::{
    collections::VecDeque,
    marker::PhantomData,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use serde::Serialize;
use tokio::{
    sync::{Mutex, mpsc, oneshot},
    time::{Instant, sleep, timeout_at},
};
use uuid::Uuid;

use crate::{
    client::Transport,
    error::{Error, Result},
    model::{BatchReceipt, ErrorEvent, Event, LogEntry, Metric},
    spool::{Spool, SpoolOptions, SpoolRecord},
};

const SERVER_MAX_BATCH_ITEMS: usize = 1_000;
const SERVER_MAX_BODY_BYTES: usize = 1_048_576;

#[derive(Clone, Debug)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 5,
            initial_backoff: Duration::from_millis(250),
            max_backoff: Duration::from_secs(15),
        }
    }
}

#[derive(Clone, Debug)]
pub struct DeliveryOptions {
    pub queue_capacity: usize,
    pub max_batch_items: usize,
    pub flush_interval: Duration,
    pub retry: RetryPolicy,
    pub spool: Option<SpoolOptions>,
}

impl Default for DeliveryOptions {
    fn default() -> Self {
        Self {
            queue_capacity: 4_096,
            max_batch_items: 256,
            flush_interval: Duration::from_secs(1),
            retry: RetryPolicy::default(),
            spool: None,
        }
    }
}

impl DeliveryOptions {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.queue_capacity == 0 {
            return Err(Error::InvalidConfiguration(
                "telemetry queue capacity must be greater than zero".into(),
            ));
        }
        if self.max_batch_items == 0 || self.max_batch_items > SERVER_MAX_BATCH_ITEMS {
            return Err(Error::InvalidConfiguration(format!(
                "telemetry batch size must be between 1 and {SERVER_MAX_BATCH_ITEMS}"
            )));
        }
        if self.flush_interval.is_zero() {
            return Err(Error::InvalidConfiguration(
                "telemetry flush interval must be greater than zero".into(),
            ));
        }
        if self.retry.initial_backoff.is_zero() {
            return Err(Error::InvalidConfiguration(
                "retry initial backoff must be greater than zero".into(),
            ));
        }
        if self.retry.max_backoff < self.retry.initial_backoff {
            return Err(Error::InvalidConfiguration(
                "retry max backoff must be greater than or equal to initial backoff".into(),
            ));
        }
        if self.retry.max_retries > 20 {
            return Err(Error::InvalidConfiguration(
                "retry max_retries must not exceed 20".into(),
            ));
        }
        if let Some(spool) = &self.spool {
            spool.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct QueueDeliveryStats {
    pub enqueued: u64,
    pub persisted: u64,
    pub recovered: u64,
    pub delivered: u64,
    pub rejected: u64,
    pub dropped: u64,
    pub deferred: u64,
    pub batches: u64,
    pub retries: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DeliveryStats {
    pub events: QueueDeliveryStats,
    pub metrics: QueueDeliveryStats,
    pub logs: QueueDeliveryStats,
    pub errors: QueueDeliveryStats,
}

#[derive(Default)]
struct QueueCounters {
    enqueued: AtomicU64,
    persisted: AtomicU64,
    recovered: AtomicU64,
    delivered: AtomicU64,
    rejected: AtomicU64,
    dropped: AtomicU64,
    deferred: AtomicU64,
    batches: AtomicU64,
    retries: AtomicU64,
}

impl QueueCounters {
    fn snapshot(&self) -> QueueDeliveryStats {
        QueueDeliveryStats {
            enqueued: self.enqueued.load(Ordering::Relaxed),
            persisted: self.persisted.load(Ordering::Relaxed),
            recovered: self.recovered.load(Ordering::Relaxed),
            delivered: self.delivered.load(Ordering::Relaxed),
            rejected: self.rejected.load(Ordering::Relaxed),
            dropped: self.dropped.load(Ordering::Relaxed),
            deferred: self.deferred.load(Ordering::Relaxed),
            batches: self.batches.load(Ordering::Relaxed),
            retries: self.retries.load(Ordering::Relaxed),
        }
    }
}

pub(crate) trait QueuedTelemetry: Serialize + Send + Sync + 'static {
    const KIND: &'static str;
    const ROUTE: &'static str;
}

impl QueuedTelemetry for Event {
    const KIND: &'static str = "events";
    const ROUTE: &'static str = "/events";
}

impl QueuedTelemetry for Metric {
    const KIND: &'static str = "metrics";
    const ROUTE: &'static str = "/metrics";
}

impl QueuedTelemetry for LogEntry {
    const KIND: &'static str = "logs";
    const ROUTE: &'static str = "/logs";
}

impl QueuedTelemetry for ErrorEvent {
    const KIND: &'static str = "errors";
    const ROUTE: &'static str = "/errors";
}

#[derive(Debug)]
struct QueuedItem {
    sequence: Option<u64>,
    payload: Vec<u8>,
}

impl From<SpoolRecord> for QueuedItem {
    fn from(record: SpoolRecord) -> Self {
        Self {
            sequence: Some(record.sequence),
            payload: record.payload,
        }
    }
}

enum QueueCommand {
    Item(QueuedItem),
    Flush(oneshot::Sender<Result<()>>),
    Shutdown(oneshot::Sender<Result<()>>),
}

pub(crate) struct DeliveryQueue<T: QueuedTelemetry> {
    sender: mpsc::Sender<QueueCommand>,
    counters: Arc<QueueCounters>,
    spool: Option<Arc<Spool>>,
    durable_enqueue_order: Mutex<()>,
    marker: PhantomData<fn() -> T>,
}

impl<T: QueuedTelemetry> DeliveryQueue<T> {
    pub(crate) async fn spawn(transport: Arc<Transport>, options: DeliveryOptions) -> Result<Self> {
        let (spool, recovered) = match options.spool.clone() {
            Some(spool_options) => {
                let (spool, recovered) =
                    Spool::open(T::KIND, spool_options, transport.spool_binding()).await?;
                (Some(spool), recovered)
            }
            None => (None, Vec::new()),
        };
        let recovered_items: Vec<QueuedItem> = recovered.into_iter().map(QueuedItem::from).collect();

        let (sender, receiver) = mpsc::channel(options.queue_capacity);
        let counters = Arc::new(QueueCounters::default());
        counters
            .recovered
            .store(recovered_items.len() as u64, Ordering::Relaxed);
        tokio::spawn(run_worker::<T>(
            receiver,
            transport,
            options,
            counters.clone(),
            spool.clone(),
            recovered_items,
        ));
        Ok(Self {
            sender,
            counters,
            spool,
            durable_enqueue_order: Mutex::new(()),
            marker: PhantomData,
        })
    }

    pub(crate) async fn enqueue(&self, item: T) -> Result<()> {
        let payload = serialize_item(&item)?;
        if let Some(spool) = &self.spool {
            let _order = self.durable_enqueue_order.lock().await;
            let permit = self
                .sender
                .clone()
                .reserve_owned()
                .await
                .map_err(|_| Error::QueueClosed { kind: T::KIND })?;
            let record = spool.append(payload).await?;
            self.counters.persisted.fetch_add(1, Ordering::Relaxed);
            permit.send(QueueCommand::Item(QueuedItem::from(record)));
            self.counters.enqueued.fetch_add(1, Ordering::Relaxed);
            return Ok(());
        }

        self.sender
            .send(QueueCommand::Item(QueuedItem {
                sequence: None,
                payload,
            }))
            .await
            .map_err(|_| Error::QueueClosed { kind: T::KIND })?;
        self.counters.enqueued.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    pub(crate) fn try_enqueue(&self, item: T) -> Result<()> {
        if self.spool.is_some() {
            return Err(Error::DurableEnqueueRequiresAsync { kind: T::KIND });
        }
        let payload = serialize_item(&item)?;
        match self.sender.try_send(QueueCommand::Item(QueuedItem {
            sequence: None,
            payload,
        })) {
            Ok(()) => {
                self.counters.enqueued.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
            Err(mpsc::error::TrySendError::Full(_)) => Err(Error::QueueFull { kind: T::KIND }),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(Error::QueueClosed { kind: T::KIND }),
        }
    }

    pub(crate) async fn request_flush(&self) -> Result<oneshot::Receiver<Result<()>>> {
        let (reply, receiver) = oneshot::channel();
        self.sender
            .send(QueueCommand::Flush(reply))
            .await
            .map_err(|_| Error::QueueClosed { kind: T::KIND })?;
        Ok(receiver)
    }

    pub(crate) async fn request_shutdown(&self) -> Result<oneshot::Receiver<Result<()>>> {
        let (reply, receiver) = oneshot::channel();
        self.sender
            .send(QueueCommand::Shutdown(reply))
            .await
            .map_err(|_| Error::QueueClosed { kind: T::KIND })?;
        Ok(receiver)
    }

    pub(crate) fn stats(&self) -> QueueDeliveryStats {
        self.counters.snapshot()
    }
}

async fn run_worker<T: QueuedTelemetry>(
    mut receiver: mpsc::Receiver<QueueCommand>,
    transport: Arc<Transport>,
    options: DeliveryOptions,
    counters: Arc<QueueCounters>,
    spool: Option<Arc<Spool>>,
    recovered: Vec<QueuedItem>,
) {
    let mut batch = recovered;
    let mut pending_error: Option<Error> = None;
    let mut deadline = if batch.is_empty() {
        None
    } else {
        let retained = flush_batch::<T>(
            &transport,
            &options,
            &counters,
            spool.as_ref(),
            &mut batch,
            &mut pending_error,
        )
        .await;
        retry_deadline(retained, &options)
    };

    loop {
        let command = match deadline {
            Some(deadline_at) => match timeout_at(deadline_at, receiver.recv()).await {
                Ok(command) => command,
                Err(_) => {
                    let retained = flush_batch::<T>(
                        &transport,
                        &options,
                        &counters,
                        spool.as_ref(),
                        &mut batch,
                        &mut pending_error,
                    )
                    .await;
                    deadline = retry_deadline(retained, &options);
                    continue;
                }
            },
            None => receiver.recv().await,
        };

        match command {
            Some(QueueCommand::Item(item)) => {
                if batch.is_empty() {
                    deadline = Some(Instant::now() + options.flush_interval);
                }
                batch.push(item);
                if batch.len() >= options.max_batch_items {
                    let retained = flush_batch::<T>(
                        &transport,
                        &options,
                        &counters,
                        spool.as_ref(),
                        &mut batch,
                        &mut pending_error,
                    )
                    .await;
                    deadline = retry_deadline(retained, &options);
                }
            }
            Some(QueueCommand::Flush(reply)) => {
                let retained = flush_batch::<T>(
                    &transport,
                    &options,
                    &counters,
                    spool.as_ref(),
                    &mut batch,
                    &mut pending_error,
                )
                .await;
                deadline = retry_deadline(retained, &options);
                let _ = reply.send(take_worker_result(&mut pending_error));
            }
            Some(QueueCommand::Shutdown(reply)) => {
                receiver.close();
                while let Some(command) = receiver.recv().await {
                    match command {
                        QueueCommand::Item(item) => batch.push(item),
                        QueueCommand::Flush(waiter) => {
                            let _ = waiter.send(Err(Error::ShuttingDown));
                        }
                        QueueCommand::Shutdown(waiter) => {
                            let _ = waiter.send(Err(Error::ShuttingDown));
                        }
                    }
                }
                let _ = flush_batch::<T>(
                    &transport,
                    &options,
                    &counters,
                    spool.as_ref(),
                    &mut batch,
                    &mut pending_error,
                )
                .await;
                let _ = reply.send(take_worker_result(&mut pending_error));
                break;
            }
            None => {
                let _ = flush_batch::<T>(
                    &transport,
                    &options,
                    &counters,
                    spool.as_ref(),
                    &mut batch,
                    &mut pending_error,
                )
                .await;
                break;
            }
        }
    }
}

async fn flush_batch<T: QueuedTelemetry>(
    transport: &Arc<Transport>,
    options: &DeliveryOptions,
    counters: &Arc<QueueCounters>,
    spool: Option<&Arc<Spool>>,
    batch: &mut Vec<QueuedItem>,
    pending_error: &mut Option<Error>,
) -> bool {
    if batch.is_empty() {
        return false;
    }

    let mut work = VecDeque::new();
    work.push_back(std::mem::take(batch));

    while let Some(mut items) = work.pop_front() {
        if items.len() > options.max_batch_items {
            let remainder = items.split_off(options.max_batch_items);
            work.push_front(remainder);
            work.push_front(items);
            continue;
        }

        counters.batches.fetch_add(1, Ordering::Relaxed);
        match send_with_retry(transport, options, counters, T::ROUTE, &items).await {
            Ok(receipt) => {
                clear_retryable_error(pending_error);
                record_receipt::<T>(counters, pending_error, &receipt);
                if let Err(error) = commit_terminal(spool, &items).await {
                    *pending_error = Some(error);
                }
            }
            Err(Error::PayloadTooLarge) if items.len() > 1 => {
                let right = items.split_off(items.len() / 2);
                work.push_front(right);
                work.push_front(items);
            }
            Err(error) if error.is_retryable() && spool.is_some() => {
                counters
                    .deferred
                    .fetch_add(items.len() as u64, Ordering::Relaxed);
                *pending_error = Some(error);
                let mut retained = items;
                while let Some(remaining) = work.pop_front() {
                    retained.extend(remaining);
                }
                *batch = retained;
                return true;
            }
            Err(error) => {
                counters
                    .dropped
                    .fetch_add(items.len() as u64, Ordering::Relaxed);
                *pending_error = Some(error);
                if let Err(commit_error) = commit_terminal(spool, &items).await {
                    *pending_error = Some(commit_error);
                }
            }
        }
    }

    *batch = Vec::with_capacity(options.max_batch_items);
    false
}

fn clear_retryable_error(pending_error: &mut Option<Error>) {
    if pending_error.as_ref().is_some_and(Error::is_retryable) {
        *pending_error = None;
    }
}

fn record_receipt<T: QueuedTelemetry>(
    counters: &QueueCounters,
    pending_error: &mut Option<Error>,
    receipt: &BatchReceipt,
) {
    counters
        .delivered
        .fetch_add(receipt.accepted as u64, Ordering::Relaxed);
    let rejected = receipt.rejected.len() as u64;
    if rejected > 0 {
        counters.rejected.fetch_add(rejected, Ordering::Relaxed);
        *pending_error = Some(Error::RejectedItems {
            kind: T::KIND,
            rejected: receipt.rejected.len(),
        });
    }
}

async fn commit_terminal(spool: Option<&Arc<Spool>>, items: &[QueuedItem]) -> Result<()> {
    let Some(spool) = spool else {
        return Ok(());
    };
    let Some(sequence) = items.last().and_then(|item| item.sequence) else {
        return Err(Error::InvalidConfiguration(
            "durable delivery item is missing its spool sequence".into(),
        ));
    };
    spool.commit_through(sequence).await
}

async fn send_with_retry(
    transport: &Arc<Transport>,
    options: &DeliveryOptions,
    counters: &QueueCounters,
    route: &str,
    items: &[QueuedItem],
) -> Result<BatchReceipt> {
    let payloads: Vec<&[u8]> = items.iter().map(|item| item.payload.as_slice()).collect();
    let mut retries = 0_u32;
    loop {
        match transport.send_serialized_batch(route, &payloads).await {
            Ok(receipt) => return Ok(receipt),
            Err(error) if error.is_retryable() && retries < options.retry.max_retries => {
                let delay = error
                    .retry_after()
                    .map(|value| value.min(options.retry.max_backoff))
                    .unwrap_or_else(|| retry_delay(&options.retry, retries));
                retries = retries.saturating_add(1);
                counters.retries.fetch_add(1, Ordering::Relaxed);
                sleep(delay).await;
            }
            Err(error) => return Err(error),
        }
    }
}

fn serialize_item<T: Serialize>(item: &T) -> Result<Vec<u8>> {
    let payload = serde_json::to_vec(item)?;
    if payload.len().saturating_add(12) > SERVER_MAX_BODY_BYTES {
        return Err(Error::PayloadTooLarge);
    }
    Ok(payload)
}

fn retry_deadline(retained: bool, options: &DeliveryOptions) -> Option<Instant> {
    retained.then(|| Instant::now() + options.retry.max_backoff)
}

fn retry_delay(policy: &RetryPolicy, retry_index: u32) -> Duration {
    let multiplier = 1_u128 << retry_index.min(20);
    let base_ms = policy
        .initial_backoff
        .as_millis()
        .saturating_mul(multiplier)
        .min(policy.max_backoff.as_millis());
    let jitter_percent = 80_u128 + (Uuid::new_v4().as_u128() % 41);
    let jittered = base_ms
        .saturating_mul(jitter_percent)
        .saturating_div(100)
        .min(policy.max_backoff.as_millis());
    Duration::from_millis(u64::try_from(jittered).unwrap_or(u64::MAX))
}

fn take_worker_result(pending_error: &mut Option<Error>) -> Result<()> {
    match pending_error.take() {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

pub(crate) async fn await_control(
    receiver: oneshot::Receiver<Result<()>>,
    kind: &'static str,
) -> Result<()> {
    receiver.await.map_err(|_| Error::QueueClosed { kind })?
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{DeliveryOptions, RetryPolicy, retry_delay};
    use crate::spool::SpoolOptions;

    #[test]
    fn validates_delivery_configuration() {
        assert!(DeliveryOptions::default().validate().is_ok());
        let mut invalid = DeliveryOptions::default();
        invalid.queue_capacity = 0;
        assert!(invalid.validate().is_err());
        invalid = DeliveryOptions::default();
        invalid.max_batch_items = 1_001;
        assert!(invalid.validate().is_err());
        invalid = DeliveryOptions::default();
        invalid.spool = Some(SpoolOptions::new("data/spool").segment_bytes(1024));
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn retry_delay_is_bounded() {
        let policy = RetryPolicy {
            max_retries: 5,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(2),
        };
        for attempt in 0..10 {
            let delay = retry_delay(&policy, attempt);
            assert!(delay <= policy.max_backoff);
            assert!(!delay.is_zero());
        }
    }
}
