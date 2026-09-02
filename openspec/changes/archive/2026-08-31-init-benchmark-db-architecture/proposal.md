## Why

executorch-bencher needs a durable, queryable record of benchmark runs across devices,
commits, and configurations before any runner or dashboard work can start. Without a
relational store with strong typing and constraints, questions like "did commit X
regress model Y on device Z" or "is this regression attributable to temperature"
can't be answered reliably, and results from incompatible configurations (different
driver versions, dirty trees, different inputs) risk being silently compared as if
equivalent. This change establishes the authoritative PostgreSQL schema and a minimal
Axum ingestion service skeleton so runners have a stable target to write results to.

## What Changes

- Add SQLx as the database access layer and Axum as the HTTP framework to `Cargo.toml`,
  using SQLx's compile-time query checking against a committed `.sqlx` offline cache.
- Add SQLx migrations implementing the core relational schema: benchmark campaigns,
  immutable device snapshots, immutable source (git) snapshots, content-addressed
  artifact metadata, run configurations with a comparability fingerprint, immutable
  per-repetition runs (with UUIDv7 primary keys), a metric catalog with normalized
  observations (created empty, no seed data), run telemetry samples, and correctness
  validations kept separate from process exit status.
- Add a minimal Axum ingestion service skeleton (crate/module structure, DB pool
  wiring, health check route) that later work will extend with actual ingestion
  endpoints. No S3/MinIO integration, no dashboard endpoints, and no runner client
  code are included in this change.
- Add a `docker-compose.yml` providing a local Postgres instance for development,
  and use `testcontainers-rs` (which requires a local Docker daemon) to provision
  ephemeral databases for integration tests.
- Establish the append-only convention: runs and their results are never updated
  in place; corrections happen as new rows, not mutations of captured provenance.
- `benchmark_campaigns.created_by` is stored as a plain `uuid` with no foreign key,
  since no user/identity system exists yet in this project — see design.md for the
  known gap this leaves. No CI pipeline is added in this change.

## Capabilities

### New Capabilities
- `benchmark-schema`: The PostgreSQL data model for campaigns, device/source
  snapshots, artifacts, run configurations, runs, metrics, telemetry, and
  validations, including the immutability and comparability-fingerprint rules
  that govern how records may be written and compared.
- `ingestion-service`: The Axum-based service skeleton (crate structure, DB
  connection pooling, configuration loading, health check) that will host the
  benchmark ingestion API in future changes.

### Modified Capabilities
(none — this is the first capability set in the project)

## Impact

- `Cargo.toml`: adds `sqlx` (postgres, uuid, chrono/time, macros, migrate,
  runtime-tokio features), `axum`, `tokio`, `uuid` (with the `v7` feature),
  `serde`/`serde_json`, and dev-dependencies `testcontainers` and a Postgres
  testcontainers module. No existing dependencies are removed.
- `src/`: reorganized from a single `main.rs` into a small module structure
  (e.g. `db`, `config`, `http`) needed to host the Axum service; existing
  `main.rs` content is replaced by the service bootstrap.
- New `migrations/` directory at the crate root holding ordered SQLx `.sql`
  migration files for the schema described above.
- New `.sqlx/` directory checked into the repo holding the offline query cache
  used for compile-time query verification without a live database.
- New `docker-compose.yml` at the crate root providing a local Postgres
  instance for development.
- No impact on other repos in the org; this work is entirely local to
  `executorch-bencher`.
