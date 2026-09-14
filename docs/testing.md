# Testing

## Layers

| Layer | Location | Access |
| --- | --- | --- |
| Unit | `src/**/*.rs` `#[cfg(test)]` modules | Private items |
| Integration | `tests/*.rs` | Public `sonde` API only |
| Shared integration helpers | `tests/common/mod.rs` | Included with `mod common;` |
| Frontend | `web/` Vitest | React components |
| Microbenchmarks | `benches/hot_path.rs` | Public API, Criterion |
| Concurrent HTTP | `tests/api_concurrency.rs` | Loopback Actix server + `reqwest` |

Do not enlarge public APIs only to make integration tests reach private helpers. Keep those tests next to the implementation.

## Rust commands

```powershell
cargo fmt --all --check
cargo check --all-targets --all-features --locked
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --all-features --locked
cargo test --test api_concurrency --locked
cargo bench --bench hot_path --locked -- --quick
```

SDK crate:

```powershell
cargo fmt --manifest-path sdk/rust/Cargo.toml --check
cargo test --manifest-path sdk/rust/Cargo.toml
```

## Integration helper

```rust
mod common;

#[tokio::test]
async fn example() {
    let database = common::memory_database().await;
    // ...
}
```

`common::memory_database()` connects to `sqlite::memory:` and runs migrations.

`common::installed_http()` writes a tempfile SQLite install, creates a Super Admin session, one application, and one ingest API key. Concurrent HTTP tests bind `127.0.0.1:0` and drive the public routes with `reqwest`.

## Conventions

- Production code: no `unwrap` / `expect`. Tests may `#![allow(clippy::unwrap_used)]` or `expect` with a reason.
- Name tests after the contract: `live_ingest_rejects_client_owned_identity_and_time_fields`.
- Keep tests deterministic: in-memory SQLite, no real network, injected time where the domain already accepts timestamps.
- One integration file per public contract (authorization, nonce replay, concurrent HTTP, rollups, backup). Do not dump unrelated cases into `sqlite_bootstrap.rs`.
- Concurrent HTTP tests (`api_concurrency.rs`) cover health, session reads, token issue, signed ingest, and nonce replay. They are contracts under parallel clients, not load tests. Numbers belong in [performance-results.md](performance-results.md).

## Frontend

```powershell
cd web
pnpm typecheck
pnpm test
pnpm build
```

Pinned toolchain: Node 22 and pnpm 10.29.3 (`web/package.json` `packageManager` and CI).
