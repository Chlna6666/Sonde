# Architecture

Sonde is a single-binary telemetry service: Actix Web serves the HTTP API and an embedded React console.

## Layers

```
api        HTTP transport: routing, request extraction, payload limits, responses
services   authorization and use-case orchestration
database   persistence, SeaORM queries, migrations
domain     transport- and database-independent types and validation
```

Dependencies flow downward. SQL and SeaORM stay out of `api`. `services` may orchestrate database modules but should not leak query builders into HTTP handlers.

## Process layout

| Path | Role |
| --- | --- |
| `src/main.rs` | Binary entry. Installs Microsoft mimalloc v3 as the process global allocator, then calls `sonde::run()`. |
| `src/lib.rs` | Library crate: HTTP server, modules, and `run()`. |
| `src/bootstrap.rs` | Load installation state and start background workers. |
| `sdk/rust` | Official ingest client (`sonde-sdk`, `publish = false`). |
| `web/` | React 19 + Vite 7 management console, embedded at release via `rust-embed`. |
| `tests/` | Integration tests against the public `sonde` API, including concurrent HTTP contracts. |
| `benches/` | Criterion microbenchmarks for ingest-adjacent hot paths. |
| `docs/performance.md` | Measurement methodology. |
| `docs/performance-results.md` | Captured Criterion and concurrent-test results. |

## Runtime shape

- Ingest is admission-controlled (`MAX_IN_FLIGHT_INGEST_REQUESTS`) and written through bounded per-type queues in `IngestWriter`.
- SQLite uses a single writer lane; MySQL/PostgreSQL use a small writer pool.
- High-throughput ingest request/byte/item buckets are process-local. Horizontally scaled deployments still need an edge or shared traffic-control layer for aggregate quotas.
- Interactive sessions, 2FA, nonce replay, and ingest bootstrap windows live in the configured database.

## Hot path

Live ingest:

1. Read a bounded body (`/api/v1/ingest/{events,metrics,logs,errors}`).
2. Verify the short-lived device token and HMAC (`sonde-hmac-sha256-v2`).
3. Validate and normalize items in `services::telemetry`.
4. Enqueue into `IngestWriter`, which coalesces same-scope batches.
5. Persist through `database::telemetry` and mark dirty rollup days.

Device identity is hashed with application + environment scope so the same source identifier cannot become a cross-app correlation key.
