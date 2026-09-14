# Sonde

[简体中文](README.zh-CN.md) | [MIT License](LICENSE)

Sonde is a self-hosted telemetry analytics platform for teams that operate more than one application. It combines a high-throughput Actix Web API with an embedded React management console in a single deployable service.

Further reading: [architecture](docs/architecture.md), [performance](docs/performance.md), [performance results](docs/performance-results.md), [testing](docs/testing.md).

## What it does

- Collects custom events, numeric metrics, and structured logs in efficient batches.
- Organizes data by application and environment, with scoped ingest keys and audit logs.
- Provides live dashboards, searchable telemetry, alert rules, and Server-Sent Events updates.
- Supports SQLite, PostgreSQL, and MySQL, with a guided first-run setup.
- Imports Cloudflare D1 SQL exports through a validated, idempotent migration flow; uploaded SQL is parsed and never executed directly.
- Includes English and Simplified Chinese UI, plus system/light/dark themes.

## Security and privacy

Sonde is designed for self-hosted deployments. Passwords use Argon2id; interactive sessions, temporary 2FA state, TOTP replay protection, ingest bootstrap rate windows, and signed-request nonce replay protection are stored in the configured database. RBAC, CSRF validation, device-bound short-lived ingest tokens, HMAC request signing, and adaptive device-risk controls are included. High-throughput ingest request/byte/item buckets and adaptive login challenges remain process-local, so horizontally scaled deployments should keep an edge or shared traffic-control layer for aggregate hot-path quotas.

Do not commit the generated `data/` directory, database files, `sonde.password-pepper`, production connection strings, ingest keys, or local `.env` files. The repository's [`.gitignore`](.gitignore) excludes these by default. Use [`.env.example`](.env.example) only as a starting point for local configuration.

## Quick start with Docker

```bash
git clone https://github.com/Chlna6666/Sonde.git
cd Sonde
docker compose up -d --build
```

Open <http://127.0.0.1:8080> and complete the initialization wizard. Docker persists application data in the `sonde_data` volume.

## Local development

Requirements: Rust 1.95.0+, Node.js 22+, and pnpm 10+.

Start the frontend with hot reload:

```powershell
cd web
pnpm install --frozen-lockfile
pnpm dev
```

In another terminal, run the backend through the Vite proxy:

```powershell
$env:SONDE_DEV_PROXY = "http://127.0.0.1:5173"
cargo run
```

Debug builds skip the embedded production frontend bundle. Set `SONDE_BUILD_WEB=1` to build it in debug mode, or use `cargo build --release` for a production build.

## Configuration

| Variable | Purpose | Default |
| --- | --- | --- |
| `SONDE_BIND` | HTTP listen address | `127.0.0.1:8080` (`0.0.0.0:8080` in Docker) |
| `SONDE_DATA_DIR` | Runtime configuration and SQLite data directory | `data` |
| `SONDE_CONFIG_PATH` | Overrides the generated configuration file location | `$SONDE_DATA_DIR/sonde.json` |
| `SONDE_PEPPER_PATH` | Overrides the generated password-pepper file location | `$SONDE_DATA_DIR/sonde.password-pepper` |
| `SONDE_DATABASE_URL` | Database connection-string override | unset |
| `SONDE_DEV_PROXY` | Vite development-server URL | unset |
| `SONDE_BUILD_WEB` | Builds frontend assets during debug builds | unset |
| `RUST_LOG` | Tracing filter | `sonde=info,actix_web=info` |

## Rust SDK

The official Rust SDK lives in `sdk/rust`. It is intentionally marked `publish = false` and is not published to crates.io. Cargo traverses a Git repository to locate the requested package, so consumers can depend on the repository root directly:

```toml
[dependencies]
sonde-sdk = { git = "https://github.com/Chlna6666/Sonde" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Pin a commit for reproducible production builds:

```toml
[dependencies]
sonde-sdk = { git = "https://github.com/Chlna6666/Sonde", rev = "<SONDE_COMMIT_SHA>" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Applications should persist one high-entropy pseudonymous installation/device identifier. Do not derive it from raw MAC addresses, hardware serial numbers, account names, or other directly identifying values.

```rust
use sonde_sdk::{Event, SondeClient, load_or_create_device_id};

#[tokio::main]
async fn main() -> sonde_sdk::Result<()> {
    let device_id = load_or_create_device_id("data/sonde-device-id")?;
    let sonde = SondeClient::builder(
        "http://127.0.0.1:8080",
        "sonde_your_bootstrap_key",
        device_id,
    )
    .app_version(env!("CARGO_PKG_VERSION"))
    .system_language("en-US")
    // Optional: persist unacknowledged telemetry across crashes and power loss.
    .disk_spool("data/sonde-spool")
    .connect()
    .await?;

    sonde
        .event(Event::new("app_startup").attribute("channel", "stable"))
        .await?;

    sonde.shutdown().await?;
    Ok(())
}
```

`load_or_create_device_id()` creates the identifier once with no-overwrite file creation and reuses it on future launches. If the existing file is malformed, the SDK reports the problem rather than silently rotating identity and turning the same installation into a new device. Put the file in the application's normal persistent data directory.

`connect()` exchanges the bootstrap key for a short-lived device token, performs an authenticated heartbeat, and starts the default 60-second heartbeat loop. The SDK handles token refresh, exact-body HMAC signing, nonces, request-signing timestamps, and four independent background queues for events, metrics, logs and errors.

Without a disk spool, async telemetry calls return after bounded-memory queue admission. Defaults are 4,096 queued items per telemetry type, 256 items per HTTP batch and a one-second automatic flush delay. Connect failures, timeouts, ambiguous responses, HTTP 429 and HTTP 5xx use exponential-backoff retries; ordinary permanent 4xx responses and explicit per-item rejections are not retried.

Enable crash-persistent delivery explicitly:

```rust
let sonde = SondeClient::builder(server, bootstrap_key, device_id)
    .disk_spool("data/sonde-spool")
    .connect()
    .await?;
```

With spooling enabled, each telemetry item is serialized, appended to its type-specific WAL, and `sync_data()`'d before entering the worker queue. Async enqueue success therefore means the item is durably stored locally; it does not mean the Sonde server has acknowledged it. Defaults are 4 MiB WAL segments and 64 MiB of disk budget **per telemetry type**.

Server terminal results advance an append-only ACK journal, and the ACK is persisted before fully acknowledged old WAL segments are reclaimed. Retryable failures that exhaust the current retry budget remain deferred in the WAL instead of being dropped; they can be retried later or recovered after restart. If the disk budget fills before acknowledged segments can be reclaimed, enqueue returns `Error::SpoolFull` rather than deleting unacknowledged telemetry.

The active segment can repair an incomplete trailing frame caused by a crash or power loss. A checksum failure inside a complete record is treated as corruption and startup fails explicitly. Each spool also holds an exclusive cross-process lock and a SHA-256 binding to the Sonde endpoint, device ID and bootstrap key, preventing concurrent writers or accidental replay into another Sonde identity. The bootstrap key is not stored in plaintext in spool metadata.

Durable delivery remains **at-least-once**: a server may accept a request before the client persists the ACK. Events requiring business-level deduplication should set `Event::idempotency_key()`. The non-blocking `try_*` APIs are intentionally unavailable in durable mode because they cannot promise a completed WAL append/fsync without waiting.

`delivery_stats()` exposes `persisted`, `recovered`, `delivered`, `rejected`, `dropped`, `deferred`, batch and retry counters for every telemetry type. Use `flush().await` for an acknowledgement barrier and `shutdown().await` during normal application shutdown.

Telemetry types intentionally do **not** expose `anonymousId`, `timestamp`, or `sessionId`. Device identity comes from the short-lived token. First/last seen, sessions, online duration, DAU/WAU/MAU, and cumulative activity are derived by Sonde from trusted server-received requests.

See [`sdk/rust/README.md`](sdk/rust/README.md) for full queue, retry and `SpoolOptions` tuning details. The underlying `sonde-hmac-sha256-v2` protocol remains an implementation reference for future SDKs in other languages; regular application code should use the Rust SDK instead of duplicating the signing protocol.

## Performance

The `sonde` binary uses Microsoft **mimalloc v3** as its process global allocator (`mimalloc` 0.1.52+, default; do not enable the crate `v2` feature). Ingest HMAC verification, scoped device hashing, SSE live updates, and writer coalescing also avoid short-lived heap buffers on the hot path. See [docs/performance.md](docs/performance.md) and captured numbers in [docs/performance-results.md](docs/performance-results.md).

```powershell
cargo bench --bench hot_path --locked
```

## Quality checks

```powershell
cargo fmt --all --check
cargo check --all-targets --all-features --locked
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --all-features --locked
cargo test --test api_concurrency --locked
cargo bench --bench hot_path --locked -- --quick

cargo fmt --manifest-path sdk/rust/Cargo.toml --check
cargo check --manifest-path sdk/rust/Cargo.toml
cargo clippy --manifest-path sdk/rust/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path sdk/rust/Cargo.toml

cd web
pnpm typecheck
pnpm test
pnpm build
```

## License

Sonde is available under the [MIT License](LICENSE).
