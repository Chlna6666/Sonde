# Performance

Sonde is a telemetry collector. Hot paths stay allocation- and query-conscious: bounded queues, batch inserts, and no unbounded result sets.

This document is the **methodology**. Captured numbers live in [performance-results.md](performance-results.md). Do not treat Criterion output as production ingest capacity.

## Allocator

The `sonde` binary installs Microsoft **mimalloc v3** as the process global allocator:

```rust
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
```

The `mimalloc` crate at **0.1.52+** defaults to v3. Do **not** enable the crate `v2` feature. mimalloc v3 reduces fragmentation for the Actix worker heap (short-lived JSON buffers, request state, ingest batches) compared with the system allocator.

The library crate does not set a global allocator so embedders can choose their own.

Release profile:

```toml
[profile.release]
lto = "thin"
codegen-units = 1
strip = true
```

## Code-level fragmentation

The allocator is not a substitute for fewer short-lived heaps:

- HMAC verification hashes the body and writes the canonical request into the MAC without building an intermediate `Vec`. Signature hex is decoded on the stack; already-uppercase HTTP methods skip the ASCII copy. Production `verify` uses `SystemTime`; benches/tests freeze time with `verify_at`.
- Device scoped hashes feed `application_id`, `:`, and `environment_id` into SHA-256 without formatting a salt `String`.
- Ingest live-update SSE payloads are written into one `String` instead of a `serde_json::Value` tree.
- Device ingest rate keys are assembled on the stack when they fit (typical UUID + device id).
- Writer lanes group same-scope requests with a small linear scan (≤ 32) instead of hashing cloned id pairs.
- Histogram encoding avoids cloning the full `HistogramInput` when the aggregate is already present. Empty bound/bucket arrays persist as shared `[]` without calling serde.
- Ingest `attributes` stay as `serde_json::value::RawValue` until a caller needs a map. Empty objects are `None` and persist as `"{}"`. Size/depth checks stream the raw JSON instead of building a `BTreeMap`.
- Explorer and error-occurrence APIs replay stored attribute JSON as `RawValue` instead of parse-to-`Value` then serialize.
- Telemetry enums store lowercase labels as `&'static str` (`as_str`) rather than `format!("{:?}", …).to_lowercase()`.

## Ingest bounds

| Limit | Value |
| --- | --- |
| HTTP ingest body | 1 MiB |
| Batch items | 1,000 |
| Writer queue | 256 requests / lane |
| Writer coalesce | 32 requests or 4,000 items / 5 ms |
| In-flight ingest permits | 64 |
| Analytics query permits | 8 |
| SQLite writer lanes | 1 |
| Remote DB writer lanes | 4 |
| Token requests / IP / minute | 30 |
| Device ingest requests / minute | 60 |

These are correctness and backpressure limits, not throughput targets.

## What we measure

| Layer | Command | What it proves | What it does not prove |
| --- | --- | --- | --- |
| Microbenchmarks | `cargo bench --bench hot_path --locked` | HMAC verify, scoped hash, validation, JSON parse, LIKE helper latency | End-to-end ingest RPS, disk, or SQL |
| Concurrent HTTP contracts | `cargo test --test api_concurrency --locked` | Health, session reads, token issue, signed ingest, and nonce replay under parallel clients | Saturated capacity or p99 under load |
| Frontend bundle | `pnpm --dir web build` | Chunk split and gzip size | Runtime chart FPS |
| Full load | not in CI | Real SQLite/Postgres/MySQL + network | — |

## How to capture Criterion numbers

Use a quiet machine. Close other heavy processes. Pin the same toolchain (`rust-toolchain.toml`).

```powershell
$env:RUSTFLAGS = "-C target-cpu=native"
cargo bench --bench hot_path --locked -- --save-baseline current
```

Quick compile-and-smoke (not for published numbers):

```powershell
cargo bench --bench hot_path --locked -- --quick
```

Criterion writes `target/criterion/`. Copy the mean / median / throughput rows into [performance-results.md](performance-results.md) together with:

- date (UTC)
- CPU, cores, RAM, OS
- `rustc --version`
- whether `target-cpu=native` was set
- profile (`bench` uses release with debug symbols unless overridden)

Compare two revisions:

```powershell
cargo bench --bench hot_path --locked -- --baseline previous --save-baseline current
```

## Load-test outline (manual)

Not automated. Against a **release** binary and a dedicated database:

1. Create one application and one ingest API key.
2. Exchange `/api/v1/ingest/token` per simulated device (stay under the 30 token-requests/IP/minute bootstrap budget, or spread source IPs).
3. POST signed `/api/v1/ingest/events` batches (≤ 1,000 items, ≤ 1 MiB) with unique nonces.
4. Record accepted RPS, HTTP 429/403 rates, SQLite WAL size, and RSS.

Suggested client: the official `sdk/rust` crate or a small `reqwest` loop. Do not disable HMAC or nonce checks when quoting ingest numbers.

For CPU profiles: `cargo flamegraph --bin sonde` on a release build with representative ingest traffic.
