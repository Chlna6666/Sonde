# Sonde Rust SDK

Official Rust client SDK for Sonde. This crate is intentionally **not published to crates.io** (`publish = false`) and is consumed directly from this Git repository.

Cargo traverses Git repositories to locate the requested crate, so the repository root URL is sufficient even though this crate lives under `sdk/rust`.

## Add the dependency

```toml
[dependencies]
sonde-sdk = { git = "https://github.com/Chlna6666/Sonde" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

For reproducible production builds, pin a commit:

```toml
[dependencies]
sonde-sdk = { git = "https://github.com/Chlna6666/Sonde", rev = "<SONDE_COMMIT_SHA>" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

## Connect

```rust
use sonde_sdk::{Event, SondeClient, load_or_create_device_id};

#[tokio::main]
async fn main() -> sonde_sdk::Result<()> {
    let device_id = load_or_create_device_id("data/sonde-device-id")?;
    let sonde = SondeClient::builder(
        "https://telemetry.example.com",
        "sonde_your_bootstrap_key",
        device_id,
    )
    .app_version(env!("CARGO_PKG_VERSION"))
    .system_language("zh-CN")
    .connect()
    .await?;

    sonde
        .event(Event::new("app_startup").attribute("channel", "stable"))
        .await?;

    sonde.shutdown().await?;
    Ok(())
}
```

`load_or_create_device_id()` creates a high-entropy pseudonymous installation ID once and reuses it on later launches. Creation is no-overwrite; concurrent first launches converge on the same persisted ID. A malformed existing identity file is reported instead of silently generating a replacement, because rotating it would make the same installation appear as a new device.

Store this file in the application's normal persistent data/config directory. Do not derive device identity from hardware serials, MAC addresses, account names, or other directly identifying values.

`connect()` performs an initial authenticated heartbeat and starts the default 60-second heartbeat loop. Sonde derives server-side first/last seen, session boundaries, online duration, DAU/WAU/MAU and cumulative activity from these trusted requests.

The SDK telemetry types intentionally do **not** expose `timestamp`, `sessionId`, or `anonymousId`. Device identity comes from the short-lived device token and telemetry time/session semantics are owned by the Sonde server.

## Reliable queued delivery

Events, metrics, logs and errors each have an independent bounded queue and background worker. The default policy is:

- 4,096 queued items **per telemetry type**.
- Up to 256 items per HTTP batch.
- Automatic flush one second after the first item enters a non-empty batch.
- Automatic split when a serialized batch exceeds Sonde's 1 MiB request limit.
- Retry connect/time-out failures, ambiguous response failures, HTTP 429 and HTTP 5xx.
- Five retries by default with exponential backoff, ±20% jitter and a 15-second maximum delay.
- Honor integer-seconds `Retry-After` for retryable responses, capped by the configured maximum backoff.
- Ordinary HTTP 4xx responses are treated as permanent failures and are not retried.
- Server-side item rejections are recorded as rejected items and are not retried.

Without a disk spool, `event().await`, `metric().await`, `log().await` and `error().await` return after bounded-memory queue admission. They apply backpressure when the queue is full. The `try_event()`, `try_metric()`, `try_log()` and `try_error()` variants never wait and return `Error::QueueFull` when there is no capacity.

Call `flush().await` when an acknowledgement barrier is required. Call `shutdown().await` from the application's normal exit path. Shutdown rejects new telemetry, drains all four queues, applies the configured retry policy and performs a final flush.

Use `delivery_stats()` to monitor the delivery path:

```rust
let stats = sonde.delivery_stats();
println!(
    "events: enqueued={} persisted={} recovered={} delivered={} rejected={} dropped={} deferred={} retries={}",
    stats.events.enqueued,
    stats.events.persisted,
    stats.events.recovered,
    stats.events.delivered,
    stats.events.rejected,
    stats.events.dropped,
    stats.events.deferred,
    stats.events.retries,
);
```

## Crash-persistent disk spool

Disk spooling is optional. Enable it when telemetry should survive process crashes, forced termination, OS crashes, or power loss:

```rust
let sonde = SondeClient::builder(server, bootstrap_key, device_id)
    .disk_spool("data/sonde-spool")
    .connect()
    .await?;
```

With disk spooling enabled, async enqueue first serializes the telemetry item, appends it to an append-only WAL and calls `sync_data()`. Only after the durable append succeeds is the item admitted to the worker queue. A successful `event().await` therefore means the item is crash-persistent locally, not that the Sonde server has already acknowledged it.

The spool is isolated per telemetry type (`events`, `metrics`, `logs`, `errors`). Defaults are 4 MiB WAL segments and a 64 MiB limit **per telemetry type**. The worker stores each serialized JSON item once and reconstructs `{"items":[...]}` directly from those bytes during replay, so recovery does not deserialize old WAL data through newer SDK model structs.

Server terminal results advance an append-only ACK journal, which is `sync_data()`'d before fully acknowledged old WAL segments can be deleted. If the server accepted a request but the client crashed before persisting the ACK, the request may be replayed. Durable delivery is therefore **at-least-once**, not exactly-once. Use `Event::idempotency_key()` where duplicate business events must be suppressed.

Retryable failures that exhaust the current retry budget are **not** removed from a durable spool. They remain deferred and are retried later; if shutdown still cannot deliver them, they remain on disk for the next process start. Permanent 4xx responses and explicit per-item server rejections are terminal and advance the durable checkpoint.

The active WAL segment tolerates an incomplete trailing frame caused by a crash and truncates it back to the last verified frame during recovery. A checksum failure inside an otherwise complete record is treated as corruption and startup fails explicitly instead of silently skipping data.

Each spool directory also has two safety guards:

- An exclusive `spool.lock` prevents two clients/processes from writing the same telemetry spool concurrently.
- `meta.bin` contains a SHA-256 binding for the Sonde endpoint, device ID and bootstrap key. Reusing an existing spool with another endpoint/device/key fails with `SpoolBindingMismatch`; the bootstrap key is not stored in plaintext.

The non-blocking `try_*` APIs are intentionally unavailable in durable mode because they cannot promise “write and fsync before success” without waiting. Use the async enqueue methods when disk spooling is enabled.

For a custom disk budget:

```rust
use sonde_sdk::SpoolOptions;

let spool = SpoolOptions::new("data/sonde-spool")
    .segment_bytes(8 * 1024 * 1024)
    .max_bytes_per_queue(128 * 1024 * 1024);

let sonde = SondeClient::builder(server, bootstrap_key, device_id)
    .spool_options(spool)
    .connect()
    .await?;
```

If the per-queue disk budget is exhausted before old acknowledged segments can be reclaimed, enqueue returns `Error::SpoolFull` rather than silently deleting unacknowledged telemetry.

### Tune delivery

```rust
use std::time::Duration;
use sonde_sdk::{DeliveryOptions, RetryPolicy, SondeClient, SpoolOptions};

let delivery = DeliveryOptions {
    queue_capacity: 8_192,
    max_batch_items: 500,
    flush_interval: Duration::from_millis(750),
    retry: RetryPolicy {
        max_retries: 6,
        initial_backoff: Duration::from_millis(200),
        max_backoff: Duration::from_secs(20),
    },
    spool: Some(
        SpoolOptions::new("data/sonde-spool")
            .segment_bytes(8 * 1024 * 1024)
            .max_bytes_per_queue(128 * 1024 * 1024),
    ),
};

let sonde = SondeClient::builder(server, bootstrap_key, device_id)
    .delivery_options(delivery)
    .connect()
    .await?;
```

The builder also provides `queue_capacity()`, `batch_size()`, `flush_interval()`, `retry_policy()`, `disk_spool()` and `spool_options()` convenience methods.

## Metrics

```rust
use sonde_sdk::Metric;

sonde
    .metric(Metric::gauge("cpu_usage_pct", 28.4).unit("%"))
    .await?;
```

## Logs

```rust
use sonde_sdk::{LogEntry, LogLevel};

sonde
    .log(LogEntry::new(LogLevel::Info, "application initialized"))
    .await?;
```

## Errors

```rust
use sonde_sdk::{ErrorEvent, ErrorSeverity};

sonde
    .error(
        ErrorEvent::new("ConfigLoadError", "failed to load config")
            .severity(ErrorSeverity::Error)
            .handled(true),
    )
    .await?;
```

## Device facts

The SDK reports the current OS family and CPU architecture by default. Additional current facts can be supplied through the builder or updated later:

```rust
use sonde_sdk::DeviceFacts;

sonde
    .set_device_facts(DeviceFacts {
        app_version: Some("1.4.2".into()),
        launcher_version: None,
        os: Some("Windows 11 24H2".into()),
        system_language: Some("zh-CN".into()),
        architecture: Some("x86_64".into()),
    })
    .await?;
```

The SDK validates device IDs, User-Agent values and device facts locally using the same limits as the Sonde live-ingest contract before sending requests.

## Protocol behavior

The SDK handles the Sonde ingest protocol internally:

- Exchanges the long-lived bootstrap API key only at `/api/v1/ingest/token`.
- Caches and refreshes short-lived `sndt_` device tokens before expiration.
- Coalesces concurrent refreshes through a single token lock and avoids redundant refreshes after a stale-token 401.
- Signs the exact serialized request bytes with `sonde-hmac-sha256-v2`.
- Generates a fresh nonce and request-signing timestamp per request.
- Retries once with a fresh token after an HTTP 401 before the delivery worker applies its transient-error policy.
- Automatically sends heartbeat requests while the client is alive.
- Supports events, metrics, logs and errors without client-controlled identity/time/session fields.
- Enforces Sonde's 1,000-item and 1 MiB batch limits before sending.
- Validates delivery receipts before advancing a durable checkpoint; malformed, incomplete, duplicate-index or truncated success responses are treated as ambiguous/retryable.

Queued delivery is **at-least-once around ambiguous transport failures**. A connection or timeout failure may occur after the server has already received a request, and a crash can occur after server acceptance but before durable ACK persistence. Events that must be deduplicated should use `Event::idempotency_key()`.
