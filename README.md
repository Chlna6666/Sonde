# Sonde

[简体中文](README.zh-CN.md) | [MIT License](LICENSE)

Sonde is a self-hosted telemetry analytics platform for teams that operate more than one application. It combines a high-throughput Actix Web API with an embedded React management console in a single deployable service.

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
```

Pin a commit for reproducible production builds:

```toml
[dependencies]
sonde-sdk = { git = "https://github.com/Chlna6666/Sonde", rev = "<SONDE_COMMIT_SHA>" }
```

Applications should persist one high-entropy pseudonymous installation/device identifier. Do not derive it from raw MAC addresses, hardware serial numbers, account names, or other directly identifying values.

```rust
use sonde_sdk::{Event, SondeClient};

#[tokio::main]
async fn main() -> sonde_sdk::Result<()> {
    let sonde = SondeClient::builder(
        "http://127.0.0.1:8080",
        "sonde_your_bootstrap_key",
        load_or_create_installation_id(),
    )
    .app_version(env!("CARGO_PKG_VERSION"))
    .system_language("en-US")
    .connect()
    .await?;

    sonde
        .event(Event::new("app_startup").attribute("channel", "stable"))
        .await?;

    Ok(())
}

fn load_or_create_installation_id() -> String {
    // Persist the first generated value in the application's own data store and reuse it.
    sonde_sdk::generate_device_id()
}
```

`connect()` exchanges the bootstrap key for a short-lived device token, performs an authenticated heartbeat, and starts the default 60-second heartbeat loop. The SDK then handles token refresh, exact-body HMAC signing, nonces, request-signing timestamps, and signed events/metrics/logs/errors internally.

Telemetry types intentionally do **not** expose `anonymousId`, `timestamp`, or `sessionId`. Device identity comes from the short-lived token. First/last seen, sessions, online duration, DAU/WAU/MAU, and cumulative activity are derived by Sonde from trusted server-received requests.

The underlying `sonde-hmac-sha256-v2` protocol remains available as an implementation reference for future SDKs in other languages. Regular application code should use the Rust SDK instead of duplicating the signing protocol.

## Quality checks

```powershell
cargo fmt --all --check
cargo check --all-targets --all-features --locked
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --all-features --locked

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
