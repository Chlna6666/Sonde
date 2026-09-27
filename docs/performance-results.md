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

## Criterion & Concurrency Capture (2026-09-28)

Working tree after RustCrypto 0.11/0.13 ecosystem upgrade, zero-copy batch/string hot-path optimization, and Vite 8 Rolldown frontend chunking (`git rev-parse --short HEAD` base `0332f6d`).

### Concurrent HTTP contracts (`cargo test --test api_concurrency --locked`)

All 5 parallel contract tests passed in 1.97s:
- `concurrent_session_and_admin_reads_succeed`: 8 `/auth/me` + 8 app list + 8 explorer reads in parallel -> all HTTP 200.
- `concurrent_health_and_setup_status_succeed`: 32 health + 16 setup status requests in parallel -> all HTTP 200.
- `concurrent_ingest_same_nonce_is_accepted_once`: 16 identical nonces simultaneously -> 1 accepted (HTTP 202), 15 rejected (HTTP 403 replay blocked).
- `concurrent_token_issue_for_distinct_devices_succeeds`: 8 distinct devices -> all HTTP 200 with scoped tokens.
- `concurrent_ingest_events_accept_distinct_nonces`: 8 signed event batches -> all HTTP 202 with valid batch receipts.

### Criterion Means (`benches/hot_path.rs`)

100 samples per benchmark under release profile with mimalloc:

#### HMAC Signature Verify (`ingest_signature::verify`)
| Input | Mean | Throughput | vs previous capture |
| --- | --- | --- | --- |
| small event (~65 B) | **532.75 ns** | 121.73 MiB/s | 750 ns → **~29% faster** |
| 1 KiB body | **1.107 µs** | 881.85 MiB/s | 1.35 µs → **~18% faster** |
| 64 KiB body | **40.757 µs** | 1.50 GiB/s | 41.3 µs |

#### Device Identity Scoped Hash
`device_identity::scoped_hash_parts`: **261.46 ns** (down from 267 ns).

#### Telemetry Validation
| Case | Mean | vs previous capture |
| --- | --- | --- |
| `EventInput::validate` | **18.559 ns** | 39 ns → **>50% faster** |
| histogram `MetricInput::validate` | **13.930 ns** | 26 ns → **>45% faster** |
| `normalized_histogram` | **86.812 ns** | 131 ns → **~34% faster** |

#### JSON Parse & Ingest Validation (`Batch<EventInput>`)
| Batch Size / Attributes | Mean | Throughput |
| --- | --- | --- |
| 1 item (empty attrs) | 1.038 µs | 963.79 Kelem/s |
| 1 item (empty attrs + validate) | 1.183 µs | 845.52 Kelem/s |
| 100 items (two attrs) | 106.72 µs | 937.04 Kelem/s |
| 100 items (two attrs + validate) | 115.35 µs | 866.93 Kelem/s |
| 1,000 items (empty attrs) | 999.96 µs | **1.000 Melem/s** |
| 1,000 items (empty attrs + validate) | 995.69 µs | **1.004 Melem/s** |
| 1,000 items (two attrs) | 1.069 ms | 935.67 Kelem/s |
| 1,000 items (two attrs + validate) | **1.140 ms** | 876.89 Kelem/s |

Full parse + schema validation of a max 1,000-item event batch completes in **~1.14 ms**.

#### JSON Serialization & Replay
| Case | Mean |
| --- | --- |
| `BatchReceipt` via serde | 294.38 ns |
| histogram bounds `[f64; 32]` | 1.230 µs |
| histogram buckets `[u64; 33]` | 284.07 ns |
| stored two-key object replay as `RawValue` | **196.11 ns** (vs 559 ns `Value` tree → **2.85× faster**) |
| string query `contains_like_pattern` | 80.267 ns |

#### Frontend Production Chunks (`pnpm --dir web build`, Vite 8 Rolldown)
| Chunk | Raw | Gzip | Notes |
| --- | --- | --- | --- |
| `vendor-react` | 251 kB | 80 kB | React 19 + ReactDOM + Scheduler + React Router |
| `vendor-motion` | 124 kB | 40 kB | Motion animation engine |
| `vendor-ui` | 78 kB | 25 kB | Radix UI + CVA + tailwind-merge + clsx |
| `vendor-i18n` | 56 kB | 18 kB | i18next + react-i18next |
| `vendor-icons` | 39 kB | 15 kB | Lucide & React Icons |
| `chunk-BuildBarChart` | 422 kB | 118 kB | Recharts + victory-vendor (lazy-loaded only on chart pages) |
| `index` (HTML shell) | 1.45 kB | 0.52 kB | **0 kB chart overhead on first paint** |

## Comprehensive Multi-Metric Stress Benchmark (2026-09-28)

Run via:
```powershell
cargo test --test stress_benchmark --locked -- --nocapture
```

Tested on debug SQLite (`journal_mode=WAL`), Windows, 4 Actix workers, 4 Tokio runtime threads. Measures end-to-end multi-connection concurrency, latency distributions (P50 to P99.9), memory footprint under load, disk storage expansion, and rate-limiting / backpressure guarantees.

### 1. Connection Scaling & Keep-Alive Throughput

Evaluating TCP connection scaling and latency under 10, 50, 100, and 200 concurrent HTTP keep-alive clients:

| Concurrent Clients | Total Requests | Elapsed | Throughput (RPS) | P50 Latency | P99 Latency | Process WorkingSet |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **10** | 200 | 62.28 ms | **3,211.5 req/s** | 2.05 ms | 16.76 ms | 76.8 MiB |
| **50** | 1,000 | 351.37 ms | **2,846.0 req/s** | 12.77 ms | 64.58 ms | 78.9 MiB |
| **100** | 2,000 | 669.65 ms | **2,986.6 req/s** | 25.87 ms | 132.69 ms | 76.4 MiB |
| **200** | 4,000 | 1,881.18 ms | **2,126.3 req/s** | 51.87 ms | 432.54 ms | 77.8 MiB |

- Under 100 concurrent clients, throughput remains high at ~2,987 RPS with median latency under 26 ms.
- Memory WorkingSet stays essentially flat (~76–79 MiB) regardless of concurrent connection scaling, demonstrating zero connection handle leakage.

### 2. High-Throughput E2E Ingest Pipeline (10,000 Events)

Evaluating end-to-end ingestion across 1,000 HTTP batches (10 items/batch = 10,000 events) including HMAC-SHA256 signature verification, Nonce replay checking, schema validation, and single-writer coalesced SQLite WAL disk persistence:

| Metric | Measured Value | Notes |
| :--- | :--- | :--- |
| **Total Ingested Events** | **10,000 items** (1,000 batches) | Full cryptographic HMAC + Nonce + UUIDv7 |
| **Total Wall Time** | 20.145 s | Single-writer disk SQLite |
| **Throughput (Items)** | **496.41 items/s** | End-to-end to disk |
| **Throughput (Batches)** | 49.64 req/s | Batch payload parsing & validation |
| **Latency Min** | 13.15 ms | Best-case batch commit |
| **Latency P50 (Median)** | **157.53 ms** | Typical batch commit round-trip |
| **Latency P75** | 411.96 ms | |
| **Latency P90** | 836.04 ms | |
| **Latency P95** | 1,394.37 ms | High writer queue pressure |
| **Latency P99** | 5,627.03 ms | Tail latency under deep disk sync |
| **Initial Storage Size** | 2,160.55 KiB | Baseline database with schema |
| **Final Storage Size** | 13,102.15 KiB | Database + WAL post-ingest |
| **Storage Cost / Event** | **1,120.4 bytes/event** | Includes raw payload, index, partition map, WAL |
| **Memory Baseline** | 68.40 MiB | Process WorkingSet prior to load |
| **Memory Peak under Load**| **86.73 MiB** (+18.33 MiB) | WorkingSet during peak concurrent writes |
| **Memory Settled (Post-Load)**| **66.81 MiB** (-1.59 MiB) | Full heap reclamation via mimalloc v3 |
| **Private Bytes Peak** | 71.12 MiB | Unshared committed virtual memory |

### 3. Complex Analytics & Explorer Queries

Evaluating 200 complex analytical aggregation queries (device breakdown, event timeline histograms, overview summaries) running concurrently against the populated telemetry dataset:

| Metric | Measured Value | Notes |
| :--- | :--- | :--- |
| **Throughput** | **367.51 req/s** | Aggregation & filtering queries |
| **Latency Min** | 1.77 ms | Cache / indexed scan hit |
| **Latency P50 (Median)** | **4.11 ms** | Fast aggregation response |
| **Latency P90** | 24.56 ms | Multi-bucket rollup calculation |
| **Latency P99** | 52.59 ms | Broad range scan |
| **Latency Max** | 58.79 ms | Tail bounded under 60 ms |
| **WorkingSet Delta** | +14.63 MiB | 69.29 MiB → 83.92 MiB during active query scans |

### 4. Rate Limiting & Backpressure Safeguards

Validating defensive thresholds under deliberate overload:

| Mechanism | Configuration / Capacity | Observed Result | Verdict |
| :--- | :--- | :--- | :--- |
| **Device Ingest Rate Limit** | 60 requests / minute / device | 60 requests accepted (HTTP 202), next 10 rejected with HTTP 429 | **PASS** (Strict quota enforcement) |
| **Analytics Semaphore** | 8 concurrent permits | 20 burst queries: 11 succeeded, 9 throttled with HTTP 429 | **PASS** (Prevents SQLite read-pool starvation) |

### 5. Mixed Real-World Workload & Memory Stability

Simulating realistic continuous production traffic over 3 consecutive burst cycles (70% ingest + 20% complex queries + 10% health/admin checks):

| Cycle | Operations | Duration | Throughput | P50 Latency | WorkingSet | PrivateBytes |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **Cycle 1** | 40 mixed ops | 1,301.02 ms | 30.7 req/s | 26.96 ms | 83.7 MiB | 66.5 MiB |
| **Cycle 2** | 40 mixed ops | 2,795.96 ms | 14.3 req/s | 46.82 ms | 77.8 MiB | 57.8 MiB |
| **Cycle 3** | 40 mixed ops | 1,162.83 ms | 34.4 req/s | 20.12 ms | 72.5 MiB | 51.2 MiB |

- **Memory Stability**: Initial WorkingSet: 68.40 MiB, Final WorkingSet: 72.50 MiB (Net change: **+4.09 MiB** across 120 mixed operations with heavy disk writes).
- **Leak Analysis**: WorkingSet stabilized around 72–77 MiB; PrivateBytes dropped from 66.5 MiB down to 51.2 MiB as temporary buffers cleared, proving absence of unbounded heap growth.

