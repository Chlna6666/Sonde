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

    // This waits only for bounded-queue admission, not for an HTTP round trip.
    sonde
        .event(Event::new("app_startup").attribute("channel", "stable"))
        .await?;

    // Long-running applications normally call this from their normal shutdown path.
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
- Retry connect/time-out failures, HTTP 429 and HTTP 5xx.
- Five retries by default with exponential backoff, ±20% jitter and a 15-second maximum delay.
- Honor integer-seconds `Retry-After` for retryable responses, capped by the configured maximum backoff.
- Ordinary HTTP 4xx responses are treated as permanent failures and are not retried.
- Server-side item rejections are recorded as rejected items and are not retried.

`event().await`, `metric().await`, `log().await` and `error().await` apply backpressure when their queue is full. They return after the item has entered the queue. For latency-sensitive call sites, `try_event()`, `try_metric()`, `try_log()` and `try_error()` return `Error::QueueFull` instead of waiting.

Call `flush().await` when an acknowledgement barrier is required. It submits flush barriers to all four workers before awaiting them, so the independent queues can finish in parallel.

Call `shutdown().await` from the application's normal exit path. Shutdown rejects new telemetry, drains all four bounded queues, retries transient failures according to the configured policy and performs a final flush. The method is idempotent.

If retries are exhausted, the in-memory batch is dropped so memory remains bounded and shutdown cannot hang indefinitely. Use `delivery_stats()` to monitor this explicitly:

```rust
let stats = sonde.delivery_stats();
println!(
    "events: enqueued={} delivered={} rejected={} dropped={} retries={}",
    stats.events.enqueued,
    stats.events.delivered,
    stats.events.rejected,
    stats.events.dropped,
    stats.events.retries,
);
```

The queue is in-memory. It improves runtime reliability and isolates application code from network latency, but it is not a crash-persistent spool. A process kill, power loss or OS crash can still lose items that have not reached Sonde.

### Tune delivery

```rust
use std::time::Duration;
use sonde_sdk::{DeliveryOptions, RetryPolicy, SondeClient};

let delivery = DeliveryOptions {
    queue_capacity: 8_192,
    max_batch_items: 500,
    flush_interval: Duration::from_millis(750),
    retry: RetryPolicy {
        max_retries: 6,
        initial_backoff: Duration::from_millis(200),
        max_backoff: Duration::from_secs(20),
    },
};

let sonde = SondeClient::builder(server, bootstrap_key, device_id)
    .delivery_options(delivery)
    .connect()
    .await?;
```

The builder also provides `queue_capacity()`, `batch_size()`, `flush_interval()` and `retry_policy()` convenience methods.

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

Queued delivery is **at-least-once around ambiguous transport failures**: a connection or timeout failure may occur after the server has already received a request. Events that must be deduplicated should use `Event::idempotency_key()`.
