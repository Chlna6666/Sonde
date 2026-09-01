use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use serde::Serialize;
use tokio::{
    sync::{mpsc, oneshot},
    time::{Instant, sleep, timeout_at},
};
use uuid::Uuid;

use crate::{
    client::Transport,
    error::{Error, Result},
    model::{BatchReceipt, ErrorEvent, Event, LogEntry, Metric},
};

const SERVER_MAX_BATCH_ITEMS: usize = 1_000;

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
}

impl Default for DeliveryOptions {
    fn default() -> Self {
        Self {
            queue_capacity: 4_096,
            max_batch_items: 256,
            flush_interval: Duration::from_secs(1),
            retry: RetryPolicy::default(),
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
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct QueueDeliveryStats {
    pub enqueued: u64,
    pub delivered: u64,
    pub rejected: u64,
    pub dropped: u64,
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
    delivered: AtomicU64,
    rejected: AtomicU64,
    dropped: AtomicU64,
    batches: AtomicU64,
    retries: AtomicU64,
}

impl QueueCounters {
    fn snapshot(&self) -> QueueDeliveryStats {
        QueueDeliveryStats {
            enqueued: self.enqueued.load(Ordering::Relaxed),
            delivered: self.delivered.load(Ordering::Relaxed),
            rejected: self.rejected.load(Ordering::Relaxed),
            dropped: self.dropped.load(Ordering::Relaxed),
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

enum QueueCommand<T> {
    Item(T),
    Flush(oneshot::Sender<Result<()>>),
    Shutdown(oneshot::Sender<Result<()>>),
}

pub(crate) struct DeliveryQueue<T: QueuedTelemetry> {
    sender: mpsc::Sender<QueueCommand<T>>,
    counters: Arc<QueueCounters>,
}

impl<T: QueuedTelemetry> DeliveryQueue<T> {
    pub(crate) fn spawn(transport: Arc<Transport>, options: DeliveryOptions) -> Self {
        let (sender, receiver) = mpsc::channel(options.queue_capacity);
        let counters = Arc::new(QueueCounters::default());
        tokio::spawn(run_worker::<T>(
            receiver,
            transport,
            options,
            counters.clone(),
        ));
        Self { sender, counters }
    }

    pub(crate) async fn enqueue(&self, item: T) -> Result<()> {
        self.sender
            .send(QueueCommand::Item(item))
            .await
            .map_err(|_| Error::QueueClosed { kind: T::KIND })?;
        self.counters.enqueued.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    pub(crate) fn try_enqueue(&self, item: T) -> Result<()> {
        match self.sender.try_send(QueueCommand::Item(item)) {
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
    mut receiver: mpsc::Receiver<QueueCommand<T>>,
    transport: Arc<Transport>,
    options: DeliveryOptions,
    counters: Arc<QueueCounters>,
) {
    let mut batch = Vec::with_capacity(options.max_batch_items);
    let mut deadline: Option<Instant> = None;
    let mut pending_error: Option<Error> = None;

    loop {
        let command = match deadline {
            Some(deadline_at) => match timeout_at(deadline_at, receiver.recv()).await {
                Ok(command) => command,
                Err(_) => {
                    flush_batch(
                        &transport,
                        &options,
                        &counters,
                        &mut batch,
                        &mut pending_error,
                    )
                    .await;
                    deadline = None;
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
                    flush_batch(
                        &transport,
                        &options,
                        &counters,
                        &mut batch,
                        &mut pending_error,
                    )
                    .await;
                    deadline = None;
                }
            }
            Some(QueueCommand::Flush(reply)) => {
                flush_batch(
                    &transport,
                    &options,
                    &counters,
                    &mut batch,
                    &mut pending_error,
                )
                .await;
                deadline = None;
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
                flush_batch(
                    &transport,
                    &options,
                    &counters,
                    &mut batch,
                    &mut pending_error,
                )
                .await;
                let _ = reply.send(take_worker_result(&mut pending_error));
                break;
            }
            None => {
                flush_batch(
                    &transport,
                    &options,
                    &counters,
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
    batch: &mut Vec<T>,
    pending_error: &mut Option<Error>,
) {
    if batch.is_empty() {
        return;
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
            Ok(receipt) => record_receipt::<T>(counters, pending_error, receipt),
            Err(Error::PayloadTooLarge) if items.len() > 1 => {
                let right = items.split_off(items.len() / 2);
                work.push_front(right);
                work.push_front(items);
            }
            Err(error) => {
                counters
                    .dropped
                    .fetch_add(items.len() as u64, Ordering::Relaxed);
                *pending_error = Some(error);
            }
        }
    }

    *batch = Vec::with_capacity(options.max_batch_items);
}

fn record_receipt<T: QueuedTelemetry>(
    counters: &QueueCounters,
    pending_error: &mut Option<Error>,
    receipt: BatchReceipt,
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

async fn send_with_retry<T: Serialize>(
    transport: &Arc<Transport>,
    options: &DeliveryOptions,
    counters: &QueueCounters,
    route: &str,
    items: &[T],
) -> Result<BatchReceipt> {
    let mut retries = 0_u32;
    loop {
        match transport.send_batch(route, items).await {
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

    #[test]
    fn validates_delivery_configuration() {
        assert!(DeliveryOptions::default().validate().is_ok());
        let mut invalid = DeliveryOptions::default();
        invalid.queue_capacity = 0;
        assert!(invalid.validate().is_err());
        invalid = DeliveryOptions::default();
        invalid.max_batch_items = 1_001;
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
