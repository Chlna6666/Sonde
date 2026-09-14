# Performance results

Captured numbers for the current tree. Reproduce with the commands in [performance.md](performance.md). These are **not** production capacity claims: Criterion is CPU-bound and single-threaded; concurrent HTTP tests are correctness under parallel clients against debug SQLite.

## Environment

| Field | Value |
| --- | --- |
| Captured | 2026-04-16 UTC |
| Host | Windows, AMD Ryzen 7 7840H (8C/16T), 32 GiB RAM |
| rustc | 1.95.0 (`59807616e 2026-04-14`) |
| Profile | Criterion `bench` (release + debug symbols); concurrent tests `test` (debug) |
| Allocator | Microsoft mimalloc v3 (`mimalloc` 0.1.52, crate `v2` feature **off**) |
| Database | tempfile SQLite (`journal_mode=WAL`) for HTTP tests |
| `RUSTFLAGS` | unset (no `target-cpu=native`) |

Re-run:

```powershell
cargo test --test api_concurrency --locked
cargo bench --bench hot_path --locked
```

## Concurrent HTTP contracts

`tests/api_concurrency.rs` binds `127.0.0.1:0`, four Actix workers, `reqwest` clients, `tokio` multi-thread runtime (4 worker threads). Debug build. All five tests passed on this capture (`cargo test --test api_concurrency --locked --offline`, 2.43 s wall).

| Test | Parallelism | Result |
| --- | --- | --- |
| `concurrent_health_and_setup_status_succeed` | 32 health + 16 setup status | all HTTP 200 |
| `concurrent_session_and_admin_reads_succeed` | 8 `/auth/me` + 8 application list + 8 explorer | all HTTP 200 |
| `concurrent_token_issue_for_distinct_devices_succeeds` | 8 distinct `deviceId` | all HTTP 200, `sndt_` tokens |
| `concurrent_ingest_events_accept_distinct_nonces` | 8 signed event batches | all HTTP 202, 8 accepted items |
| `concurrent_ingest_same_nonce_is_accepted_once` | 16 identical nonce | 1 accepted, 15 HTTP 403 |

What this proves: health, session reads, token issue, signed ingest, and nonce uniqueness remain correct when many clients hit the same process. What it does **not** prove: saturated RPS, p99 latency, or PostgreSQL/MySQL behaviour.

## Criterion (`benches/hot_path.rs`)

Means from Criterion 0.5 (100 samples unless noted). Throughput is Criterion's conversion of mean time, not an ingest pipeline.

### HMAC verify (`ingest_signature::verify`)

Canonical request is streamed into the MAC (no intermediate `Vec`).

| Input | Mean | Throughput |
| --- | --- | --- |
| small event JSON (~65 B) | 870 ns | 75 MiB/s |
| 1 KiB body | 1.47 µs | 666 MiB/s |
| 64 KiB body | 41.8 µs | 1.46 GiB/s |

SHA-256 of the body dominates as size grows. 64 KiB is still well under the 1 MiB ingest cap.

### Device scoped hash

`device_identity::scoped_hash_parts` (value \|\| `app` \|\| `:` \|\| `env` into SHA-256): **289 ns**.

### Telemetry validation

| Case | Mean |
| --- | --- |
| `EventInput::validate` | 25.2 ns |
| histogram `MetricInput::validate` | 19.4 ns |
| `normalized_histogram` (clone/or-legacy) | 110 ns |

### JSON parse of event batches

`serde_json::from_slice::<Batch<EventInput>>` of an already-serialized batch:

| Items | Mean | Throughput |
| --- | --- | --- |
| 1 | 1.21 µs | 827 Kelem/s |
| 100 | 117 µs | 856 Kelem/s |
| 1,000 (max batch) | 1.18 ms | 846 Kelem/s |

Parse of a full 1,000-item batch is about 1.2 ms on this CPU **before** HMAC, token verify, SQLite, or writer coalesce.

### Query helper

`contains_like_pattern("win%_\\dows")`: **71 ns**.

## Frontend production chunks

`pnpm --dir web build` (Vite 7), same tree. Gzip sizes:

| Chunk | Raw | Gzip |
| --- | --- | --- |
| `vendor-react` | 232 kB | 74 kB |
| `vendor-charts` (recharts) | 414 kB | 118 kB |
| `vendor-motion` | 127 kB | 42 kB |
| `vendor-i18n` | 49 kB | 16 kB |
| `index` (shell) | 51 kB | 16 kB |
| `locale-zh-CN` / `locale-en` | 27 kB each | 11 / 9 kB |

Charts and motion stay out of the first paint of login/setup.

## Interpreting the numbers

- HMAC + JSON parse of a 1,000-item batch is on the order of **a millisecond plus SHA-256 of the body**. Persistence, WAL, and rollup dirty-day writes dominate real ingest.
- SQLite uses **one writer lane**. Concurrent ingest tests pass because the writer queue coalesces; they will not scale linearly with Actix workers on SQLite.
- Process-local request/byte/item buckets still apply (60 device requests / minute, 120 IP requests / minute). A load generator that ignores those budgets will measure 429s, not peak ingest.
- Token bootstrap is a separate, stricter budget (30 IP token requests / minute). Spread devices or IPs when load-testing token issue.

When you change a hot path, append a new dated section rather than silently rewriting this one, and note the git revision.

## HMAC / ingest JSON (2026-04-16, uncommitted)

Same host and rustc as the section above. Working tree after HMAC `verify_at` + delayed `attributes` parse (`git rev-parse --short HEAD` was `5db9d24`; this capture includes uncommitted hot-path changes). Criterion filter `ingest_`, then `device_identity` / `telemetry_` / `contains_like`. `RUSTFLAGS` unset.

What changed relative to the first Criterion table:

- HMAC benches call `ingest_signature::verify_at` with a frozen timestamp, so `Utc::now()` / `SystemTime` is not in the loop.
- Signature hex uses a stack `[u8; 32]` decoder (mixed-case, no `hex` crate on the verify path).
- `Attributes` deserialize as `Box<RawValue>`; empty `{}` is `None`. Persistence writes the original JSON. Validation streams the raw object instead of allocating `serde_json::Map` (`BTreeMap`).
- JSON benches now split empty objects vs two keys, and also measure parse+`validate`.

### HMAC verify (`ingest_signature::verify_at`)

| Input | Mean | Throughput | vs first capture |
| --- | --- | --- | --- |
| small event JSON (~65 B) | 750 ns | 86 MiB/s | 870 ns → ~14% faster (clock no longer in the loop) |
| 1 KiB body | 1.35 µs | 721 MiB/s | 1.47 µs |
| 64 KiB body | 41.3 µs | 1.48 GiB/s | 41.8 µs (SHA-256 of the body still dominates) |

### JSON parse of event batches

`serde_json::from_slice::<Batch<EventInput>>` only:

| Payload | 1 item | 100 items | 1,000 items |
| --- | --- | --- | --- |
| empty `attributes` | 1.02 µs (979 Kelem/s) | 103 µs (974 Kelem/s) | 1.02 ms (978 Kelem/s) |
| two keys (`channel`, `build`) | 1.06 µs (943 Kelem/s) | 108 µs (928 Kelem/s) | 1.16 ms (862 Kelem/s) |

Parse + `EventInput::validate` for two-key batches: **1.20 ms / 1,000 items**. Streaming attribute limits add ~40 µs on a full batch; empty attributes add almost nothing.

The first capture's 1.18 ms / 1,000 items was empty attributes parsed into `BTreeMap`. Empty objects are now ~1.02 ms. Two-key payloads stay in the same 1.2 ms band because keys are not materialized until `decoded()` / dimension extraction.

### Other `hot_path` means (same session)

| Case | Mean |
| --- | --- |
| `device_identity::scoped_hash_parts` | 267 ns |
| `EventInput::validate` (empty attrs) | 39 ns (tens of ns; noisy vs 25 ns) |
| histogram `MetricInput::validate` | 26 ns |
| `normalized_histogram` | 131 ns |
| `contains_like_pattern` | 55 ns |

HMAC + JSON parse of a 1,000-item two-attribute batch is still **about a millisecond plus SHA-256 of the body**. Persistence still dominates real ingest.

## JSON serialize / replay (2026-04-16, uncommitted)

Same host. Filter `ingest_json_serialize`. Hand-rolled `BatchReceipt` / `u64` array encoders were **slower** than `serde_json` on this CPU and were **not** kept on the production path. Empty numeric arrays still skip serde (`[]`).

Typed ingest responses stay on `HttpResponse::json`. Explorer / error occurrence `attributes` now serialize as `RawValue` (stored bytes in, same bytes out).

| Case | Mean |
| --- | --- |
| `BatchReceipt` via serde | 470 ns |
| histogram bounds `[f64; 32]` via serde | 1.80 µs |
| histogram buckets `[u64; 33]` via serde | 347 ns |
| stored two-key object replay as `RawValue` | **242 ns** |
| same object parse-to-`Value` then serialize | 619 ns |

Raw replay is about **2.6×** faster than building a `serde_json::Value` tree. That is the serialize win for Explorer/error pages.

Do not replace `serde_json` with SIMD parsers for ingest: HMAC already hashed the body, so a mutating parser cannot reuse that buffer, and `unsafe_code = "forbid"` rules out `simd-json`. Keep typed serde + delayed `RawValue` attributes.
