# AGENTS.md

## Scope

These rules apply to the entire Sonde repository unless a deeper `AGENTS.md` explicitly overrides them.

## Toolchain

- Rust edition: **2024**.
- Rust toolchain/MSRV: **1.95.0**.
- Keep `Cargo.toml`, `rust-toolchain.toml`, CI, formatting, Clippy, and tests aligned with Rust 1.95.0.
- Do not introduce syntax, APIs, or dependencies whose MSRV exceeds Rust 1.95.0.

## Development-stage compatibility policy

Sonde is currently in active development and does **not** preserve compatibility for obsolete internal APIs, module names, backup formats, or unreleased database layouts unless a task explicitly requires it.

- Prefer one canonical implementation over compatibility wrappers.
- Remove obsolete aliases, deprecated functions, duplicate routes, and old format readers instead of carrying them forward.
- Do not add `legacy`, `compat`, `old`, `v2`, `v3`, or similar names merely to preserve superseded development-stage behavior.
- Version identifiers are acceptable only where they are part of an actual external protocol, serialized format, immutable migration identity, or third-party API.

## Rust naming conventions

Names must describe domain responsibility, not implementation mechanics.

### Modules and files

- Use concise domain nouns or noun phrases: `applications`, `auth`, `telemetry`, `rollups`, `backup`, `device_query`.
- Do **not** suffix modules/files with implementation-role words such as `_repo`, `_repository`, `_impl`, `_service_impl`, `_manager_impl`, or `_handler_impl`.
- Do **not** suffix current modules/files/types/functions with development-history versions such as `_v2` or `_v3`.
- Do **not** prefix current code with `legacy_` unless it genuinely implements a still-required external legacy protocol.
- Prefer a directory module when one domain needs several cohesive implementation files, e.g. `database/backup/{mod.rs,format.rs,restore.rs,validation.rs}`.
- Use Rust's standard module discovery (`mod child;` with `parent/child.rs` or `parent/child/mod.rs`) for normal source modules.
- Do **not** use `#[path = "..."]` to connect ordinary production modules. Reserve `#[path]` for exceptional generated/platform/test compilation cases where standard module discovery cannot express the layout, and document the reason at the declaration.

### Types and functions

- Public types and functions must use version-neutral names when only one supported implementation exists.
- Prefer `BackupError`, `BackupRecord`, `BackupManifest`, `export_system`, `restore_system` over `BackupV2Error`, `BackupV2Record`, `export_system_v2`, etc.
- Avoid leaking persistence-layer terminology into service/API layers.
- Avoid aliases whose only purpose is to preserve an obsolete development-stage identifier.

## Architecture boundaries

- `api`: HTTP transport only—routing, request extraction, response construction, transport-specific limits.
- `services`: authorization and application use cases/orchestration.
- `database`: persistence operations and database-specific models/queries.
- `domain`: transport- and database-independent domain types/rules.
- Keep SQL/SeaORM details out of `api` and preferably out of `services`.
- Do not create duplicate implementations of the same use case across API pages or routes.

## Database code

- With SeaORM/SeaQuery traits in scope, avoid ambiguous primitive method syntax such as `value.max(...)` / `value.min(...)` when trait methods can collide. Use `std::cmp::max`, `std::cmp::min`, `clamp`, or an explicitly typed equivalent.
- Keep query limits bounded and non-zero where required.
- Prefer transactions for multi-table state transitions that must be atomic.
- Do not silently ignore database errors.

## Backup policy

- Maintain exactly one current full-system backup format and one canonical pair of HTTP routes during development.
- Current backup export/restore must be streaming/bounded-memory for large telemetry datasets.
- Restore must validate framing, manifest, record semantics, digest/count integrity, and only mutate the target after validation succeeds.
- Do not accept obsolete Sonde backup versions during development unless explicitly requested.
- Do not retain duplicate JSON and NDJSON full-system backup APIs.

## Error handling

- No `unwrap()` / `expect()` in production paths unless an invariant is statically guaranteed and documented.
- Propagate or map errors with meaningful context.
- Do not turn recoverable failures into panics.
- API errors must not expose secrets or raw internal credentials.

## Performance

Sonde is a telemetry system; hot paths must remain allocation- and query-conscious.

- Avoid unnecessary cloning, buffering entire result sets, N+1 queries, and unbounded collections.
- Prefer batching, cursor/keyset pagination, streaming, and bounded queues.
- Preserve backpressure on ingestion/export paths.
- Avoid blocking filesystem/network/database work on async executor threads.

## Validation before committing

Run the equivalent of:

```text
cargo fmt --all --check
cargo check --all-targets --all-features --locked
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --all-features --locked
```

Optional microbenchmarks live in `benches/` and are run with `cargo bench --bench hot_path --locked`. Concurrent HTTP contracts live in `tests/api_concurrency.rs`. Recorded numbers belong in `docs/performance-results.md`.

For `web/` changes also run typecheck, tests, and production build with the repository-pinned Node/pnpm versions.

## Change discipline

- Update all call sites in the same change when renaming an internal API.
- Delete dead compatibility code instead of leaving commented-out or deprecated copies.
- Tests must use canonical current names and current formats.
- Keep `main` buildable after each pushed commit; use atomic Git trees for broad renames when possible.
