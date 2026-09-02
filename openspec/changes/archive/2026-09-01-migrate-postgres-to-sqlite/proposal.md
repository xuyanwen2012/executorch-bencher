## Why

The project has no production data yet — only a stub PostgreSQL schema and
service skeleton from the prior architecture change, plus a `docker-compose.yml`/
`testcontainers-rs` dependency on a Postgres server. Running benchmarks against
Android devices from a laptop/workstation does not need a networked database
server, S3/MinIO, or TimescaleDB: it needs a self-contained backend that starts
with zero external setup. This change replaces PostgreSQL with embedded SQLite
and narrows the schema to the smallest model that captures immutable
benchmark-run evidence, deferring campaigns, metric catalogs, telemetry
time-series, and validations until a real need for them is demonstrated.

## What Changes

- **BREAKING**: Replace the PostgreSQL data store with an embedded SQLite
  database file (`data/benchmarks.sqlite3`), accessed only by the backend
  process. SQLx's `postgres` feature is replaced with `sqlite`; `uuid` and
  Postgres-specific `chrono`/enum/jsonb usage are replaced with SQLite-
  compatible representations.
- **BREAKING**: Replace the normalized multi-table schema (`benchmark_campaigns`,
  `device_snapshots`, `source_snapshots`, `run_configurations`, `run_status`
  enum, `runs`, `metrics`, `run_metrics`, `run_telemetry`, `validations`) with
  a small MVP schema of `runs`, `artifacts`, and `schema_metadata`. Device
  state, performance configuration, and build/workload identity become
  columns captured directly on each immutable `runs` row instead of separate
  referenced snapshot/configuration tables.
- Every SQLite connection is configured with `PRAGMA foreign_keys = ON`,
  `journal_mode = WAL`, `busy_timeout = 5000`, and `synchronous = FULL`,
  applied per-connection where SQLite requires it (not database-persistent).
- Run IDs are generated in the Rust application (UUIDv7) instead of relying on
  a PostgreSQL UUID function.
- Timestamps are stored as UTC RFC 3339 text consistently across schema, Rust
  types, and tests.
- Command-line argument arrays, input parameters, and environment-variable
  allowlist captures are stored as canonical JSON text columns, validated at
  the application boundary before insertion.
- Performance configuration is narrowed to exactly three pinned clocks - GPU
  clock, MIF (memory-interface) clock, and INT (interconnect) clock - stored
  as plain `INTEGER` columns in MHz with documented defaults (980, 5333, and
  934 respectively), rather than an open-ended CPU-cluster frequency map. No
  CPU-cluster, NPU, DSP, or general memory-frequency fields are part of the
  MVP.
- Exit status and correctness result use `CHECK`-constrained `TEXT` columns
  instead of a PostgreSQL `ENUM` type, and are recorded as independent
  columns on `runs` so a successful process exit and a failed correctness
  check can coexist.
- Artifacts (stdout, stderr, crash logs) are written to a local
  content-addressed directory (`data/artifacts/sha256/<aa>/<sha256>`) using a
  write-temp-file → hash-verify → atomic-rename sequence, replacing the prior
  opaque `storage_uri` design aimed at S3/MinIO. `artifacts` rows store a
  path relative to the configured artifact root, never an absolute or
  machine-specific path, and are deduplicated on `(sha256, size_bytes)`.
- Remove `docker-compose.yml`'s PostgreSQL service, the `testcontainers`/
  `testcontainers-modules` dev-dependencies, and Postgres-specific
  documentation in `README.md`; replace with SQLite-only local dev setup
  (temp files, no external service).
- The prior PostgreSQL migrations and their still-empty schema are deleted
  and replaced with a single SQLite-compatible initial migration set, since
  the project has not shipped and holds no data worth preserving.

## Capabilities

### New Capabilities
(none)

### Modified Capabilities
- `benchmark-schema`: Replace the normalized campaign/snapshot/configuration/
  metrics/telemetry/validation model with an MVP model centered on immutable
  `runs` rows (metadata, device-state snapshot, performance configuration,
  build/workload identity, and results as columns) plus a content-addressed
  local-filesystem `artifacts` table. Device state and performance
  configuration remain point-in-time snapshots, now captured directly per-run
  rather than through a separate referenced entity. Correctness/validation
  outcome remains independent of process exit status, now as a column on
  `runs` instead of a separate `validations` table. Metrics/telemetry
  catalogs, campaigns, and run-configuration comparability fingerprints are
  removed from this capability's scope.
- `ingestion-service`: Replace the PostgreSQL connection pool and Postgres
  reachability semantics with an embedded SQLite connection pool opened
  against a local database file, including per-connection pragma
  configuration (foreign keys, WAL, busy timeout, synchronous). Startup and
  health-check requirements are restated in terms of the local database file
  being openable/writable rather than a remote server being reachable.

## Impact

- **Code**: `src/db.rs`, `src/config.rs`, `src/http.rs`, `src/lib.rs`,
  `src/main.rs` (new artifact-storage module), `migrations/*` (replaced),
  `tests/*` (rewritten to use temp SQLite files instead of testcontainers).
- **Dependencies**: `Cargo.toml` — drop `sqlx` `postgres` feature and add
  `sqlite`; drop `testcontainers`/`testcontainers-modules`; add a SHA-256
  hashing crate (e.g. `sha2`) for artifact content-addressing if not already
  transitively available.
- **Configuration**: `.env`/`DATABASE_URL` now points at a local SQLite file
  path instead of a `postgres://` connection string; add an artifact-root
  configuration value.
- **Dev infrastructure**: `docker-compose.yml` is removed (no external
  service needed); `README.md`'s SQLx offline-cache instructions are updated
  to the SQLite workflow; `.gitignore` gains an entry for the local `data/`
  directory (the SQLite file and artifact blobs are runtime state, not
  source).
- **CI**: the repository has no CI configuration yet, so there is nothing to
  update; if one is added later it needs no Postgres/Docker service, since
  tests use temporary SQLite files.
