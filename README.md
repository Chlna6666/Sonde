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

Sonde is designed for self-hosted deployments. Passwords use Argon2id; interactive sessions, temporary 2FA state, and TOTP replay protection are stored in the configured database. RBAC, CSRF validation, and adaptive login protection are included. Ingest rate limits, login challenges, and ingest nonce replay caches are currently process-local, so horizontally scaled deployments should use a consistent routing or shared enforcement layer for those controls.

Do not commit the generated `data/` directory, database files, `sonde.password-pepper`, production connection strings, ingest keys, or local `.env` files. The repository's [`.gitignore`](.gitignore) excludes these by default. Use [`.env.example`](.env.example) only as a starting point for local configuration.

## Quick start with Docker

```bash
git clone https://github.com/Chlna6666/Sonde.git
cd Sonde
docker compose up -d --build
```

Open <http://127.0.0.1:8080> and complete the initialization wizard. Docker persists application data in the `sonde_data` volume.

## Local development

Requirements: Rust 1.94+, Node.js 22+, and pnpm 10+.

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

## Ingest telemetry

Create an application in the console, then copy its ingest key once.

```bash
curl -X POST http://127.0.0.1:8080/api/v1/ingest/events \
  -H "Authorization: Bearer sonde_..." \
  -H "Content-Type: application/json" \
  -d '{"items":[{"name":"application.start","anonymousId":"client-1","appVersion":"2.0.0","os":"Windows","attributes":{}}]}'
```

Batches accept 1–1000 items and are limited to 1 MiB. Invalid items return their indexes while valid entries in the same request can still be stored.

## Quality checks

```powershell
cargo fmt --all --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets
cd web
pnpm typecheck
pnpm test
pnpm build
```

## License

Sonde is available under the [MIT License](LICENSE).
