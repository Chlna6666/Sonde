# Sonde Rust SDK

Official Rust client SDK for Sonde. This crate is intentionally **not published to crates.io** (`publish = false`) and is consumed directly from this Git repository.

Cargo traverses Git repositories to locate the requested crate, so the repository root URL is sufficient even though this crate lives under `sdk/rust`.

## Add the dependency

```toml
[dependencies]
sonde-sdk = { git = "https://github.com/Chlna6666/Sonde" }
```

For reproducible production builds, pin a commit:

```toml
[dependencies]
sonde-sdk = { git = "https://github.com/Chlna6666/Sonde", rev = "<SONDE_COMMIT_SHA>" }
```

## Connect

```rust
use sonde_sdk::{Event, SondeClient};

#[tokio::main]
async fn main() -> sonde_sdk::Result<()> {
    let sonde = SondeClient::builder(
        "https://telemetry.example.com",
        "sonde_your_bootstrap_key",
        load_or_create_installation_id(),
    )
    .app_version(env!("CARGO_PKG_VERSION"))
    .system_language("zh-CN")
    .connect()
    .await?;

    sonde
        .event(Event::new("app_startup").attribute("channel", "stable"))
        .await?;

    Ok(())
}

fn load_or_create_installation_id() -> String {
    // On the first launch, persist this value in the application's own config/data store.
    // Reuse it on later launches. Do not derive it from hardware serials or account names.
    sonde_sdk::generate_device_id()
}
```

`connect()` performs an initial authenticated heartbeat and starts the default 60-second heartbeat loop. Sonde derives server-side first/last seen, session boundaries, online duration, DAU/WAU/MAU and cumulative activity from these trusted requests.

The SDK telemetry types intentionally do **not** expose `timestamp`, `sessionId`, or `anonymousId`. Device identity comes from the short-lived device token and telemetry time/session semantics are owned by the Sonde server.

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

## Protocol behavior

The SDK handles the Sonde ingest protocol internally:

- Exchanges the long-lived bootstrap API key only at `/api/v1/ingest/token`.
- Caches and refreshes short-lived `sndt_` device tokens before expiration.
- Signs the exact serialized request bytes with `sonde-hmac-sha256-v2`.
- Generates a fresh nonce and request-signing timestamp per request.
- Retries once with a fresh token after an HTTP 401.
- Automatically sends heartbeat requests while the client is alive.
- Supports events, metrics, logs and errors without client-controlled identity/time/session fields.
- Enforces Sonde's 1,000-item and 1 MiB batch limits before sending.
