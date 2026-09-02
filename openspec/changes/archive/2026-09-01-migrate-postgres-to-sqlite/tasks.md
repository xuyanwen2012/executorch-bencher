## 1. Dependencies and dev infrastructure

- [x] 1.1 Update `Cargo.toml`: replace `sqlx`'s `postgres` feature with
  `sqlite`, drop the `uuid` sqlx feature if postgres-specific, add the `sha2`
  crate for artifact hashing and `tempfile` as a dev-dependency; remove
  `testcontainers`/`testcontainers-modules` from `[dev-dependencies]`. Verify
  with `cargo metadata --no-deps` that only the expected dependency set
  resolves.
- [x] 1.2 Delete `docker-compose.yml` (no external service needed). Verify
  the file no longer exists and no remaining doc references it.
- [x] 1.3 Update `.env`/example env to a local SQLite file path (e.g.
  `DATABASE_URL=sqlite://data/benchmarks.sqlite3`) and add an artifact-root
  configuration value (e.g. `ARTIFACT_ROOT=data/artifacts`). Verify by
  inspecting the file contents.
- [x] 1.4 Update `README.md`'s SQLx offline-cache instructions to the
  SQLite workflow (no `docker compose up`, point `DATABASE_URL` at a local
  file, run `cargo sqlx migrate run` then `cargo sqlx prepare`). Verify by
  following the updated steps and confirming `cargo build` succeeds with
  `SQLX_OFFLINE=true`.
- [x] 1.5 Add `/data` to `.gitignore` (the SQLite database file and artifact
  blobs are local runtime state, not source). Verify `git status` shows a
  locally created `data/` directory as ignored, not untracked.

## 2. Migrations

- [x] 2.1 Delete the nine existing PostgreSQL migration file pairs under
  `migrations/`. Verify `ls migrations/` shows no `.sql` files referencing
  `uuid`, `jsonb`, `timestamptz`, or `CREATE TYPE`.
- [x] 2.2 Add a new SQLite-compatible migration creating `runs` with all
  MVP columns (run metadata, device-state snapshot fields, performance
  configuration fields, build/workload identity fields, results fields) per
  `design.md` - Decisions, including `NOT NULL`, `CHECK` constraints for
  exit status, correctness result, nonnegative counts/frequencies/temperatures,
  and SHA-256 length, and indexes for start time, device serial, git commit,
  BSP version, SUMD driver version, exit status, correctness result,
  executable hash, model hash, and input hash. Verify by running the
  migration against a fresh temp SQLite file and inspecting `.schema runs`.
- [x] 2.3 Add a migration creating `artifacts` (id, sha256, size_bytes, kind,
  original_filename nullable, storage_path, media_type nullable, created_at,
  metadata JSON nullable) with a `UNIQUE (sha256, size_bytes)` constraint and
  a `CHECK` on `kind` for `stdout`/`stderr`/`crash_log`. Verify by inspecting
  `.schema artifacts` against a fresh temp SQLite file.
- [x] 2.4 Add the foreign-key columns from `runs` to `artifacts`
  (stdout/stderr/crash-log artifact references, nullable) with
  `REFERENCES artifacts (id)`, either in the `runs` migration or a follow-up
  migration. Verify a foreign-key-violating insert is rejected against a
  migrated temp database with `PRAGMA foreign_keys = ON`.
- [x] 2.5 Add a migration creating `schema_metadata` (single-row schema
  version record) and have it populate the initial version. Verify by
  querying `schema_metadata` after migrating a fresh temp database.
- [x] 2.6 Run `openspec` migration verification: apply all migrations to a
  brand-new empty SQLite file and confirm no errors; run again on the same
  file to confirm idempotency (already-applied migrations are skipped).

## 3. Database connection and configuration

- [x] 3.1 Rewrite `src/config.rs` to parse the SQLite database file path and
  artifact-root path from environment variables, replacing the
  Postgres-connection-string expectation. Verify existing config unit tests
  (updated for SQLite) pass.
- [x] 3.2 Rewrite `src/db.rs`: replace `PgPool`/`PgPoolOptions` with
  `SqlitePool` built via `SqlitePoolOptions::connect_with` and
  `SqliteConnectOptions` configured with `foreign_keys(true)`,
  `busy_timeout(Duration::from_millis(5000))`,
  `synchronous(SqliteSynchronous::Full)`, `journal_mode(SqliteJournalMode::Wal)`,
  and `create_if_missing(true)`; keep `sqlx::migrate!().run(&pool)` on
  startup. Verify `connect_and_migrate` unit test asserts a clear error when
  the database file path cannot be opened/created (e.g. a nonexistent parent
  directory with no create permission).
- [x] 3.3 Update `ping`/health-check query in `src/db.rs` to use SQLite
  syntax (`SELECT 1`) via `SqlitePool`. Verify the existing health-check
  unit test still compiles and passes against a SQLite pool.
- [x] 3.4 Add a test that opens a second pooled connection after startup
  and confirms `PRAGMA foreign_keys` and `PRAGMA busy_timeout` are set on it
  too (not just the first connection), per the "newly opened pooled
  connection enforces the same settings" spec scenario.

## 4. Artifact storage

- [x] 4.1 Create `src/artifact_store.rs` implementing: write incoming bytes
  to a temp file under `<artifact_root>/tmp/`, stream-hash with SHA-256,
  atomically rename to `<artifact_root>/sha256/<first-2-hex>/<full-hash>`,
  then insert or reuse the `artifacts` row keyed on `(sha256, size_bytes)`.
  Verify with a unit test that two writes of identical content produce one
  `artifacts` row and one stored file.
- [x] 4.2 Add `pub mod artifact_store;` to `src/lib.rs`. Verify `cargo build`
  succeeds.
- [x] 4.3 Ensure the artifact store never leaves a dangling database
  reference: verify with a test that simulates a failure between hashing and
  rename (e.g. by pointing at a non-writable destination directory) and
  confirms no `artifacts` row was inserted.
- [x] 4.4 Ensure `storage_path` is always recorded relative to the
  configured artifact root, never absolute. Verify with a unit test
  asserting the stored path does not start with the artifact root's own
  absolute prefix.

## 5. Repository layer and domain types

- [x] 5.1 Add Rust domain types for exit status and correctness result
  (enums mapped to the `CHECK`-constrained `TEXT` columns via `sqlx::Type`
  or manual `Encode`/`Decode`), rejecting any value outside the fixed
  vocabulary at construction. Verify a unit test constructing an invalid
  variant from a raw string fails.
- [x] 5.2 Add a `Sha256Hex` newtype validating 64-character lowercase hex at
  construction (`TryFrom<String>` or similar). Verify unit tests for valid,
  too-short, uppercase, and non-hex inputs.
- [x] 5.3 Add JSON validation helpers for command-line argument arrays,
  input parameters, and environment-variable allowlist captures, producing
  canonical JSON text and rejecting malformed input before any insert.
  Verify unit tests for valid and malformed JSON inputs.
- [x] 5.3a Performance configuration is three fixed scalar clocks (GPU, MIF,
  INT clock in MHz, defaults 980/5333/934) enforced entirely by the
  database `CHECK (... > 0)` constraints in the `runs` migration - no JSON
  validation helper is needed (see design.md - "Performance configuration:
  three fixed clock columns, not a frequency map"). Verified by the
  migration's `CHECK` constraints and by task 6.10's negative-frequency
  test.
- [x] 5.4 Add a `runs` repository module with functions to insert a
  complete run and fetch a run by ID, using validated domain types and
  going through `artifact_store` for any artifact references. Verify with
  the integration tests in section 6.
- [x] 5.5 Ensure no repository or handler function returns a raw filesystem
  path in a way that could reach an HTTP response; only artifact IDs are
  exposed. Verify by inspecting response types/serialization for any
  `storage_path` field (there should be none reachable from the public API).

## 6. Tests

- [x] 6.1 Rewrite `tests/common/mod.rs` to provision a temporary SQLite file
  (via `tempfile`) and a temporary artifact-root directory, run migrations,
  and return a `SqlitePool` plus the temp directory guards - replacing the
  `testcontainers`-based `migrated_pool`. Verify `tests/migrations.rs` (now
  targeting SQLite) passes.
- [x] 6.2 Add/update a seed helper inserting a complete run (all MVP fields
  populated) for reuse across tests, replacing the old
  campaign/device-snapshot/source-snapshot/configuration seed chain.
- [x] 6.3 Test: creating and retrieving a complete run round-trips all
  fields correctly.
- [x] 6.4 Test: creating a run with a null decode speed succeeds and reads
  back as `None`.
- [x] 6.5 Test: recording a crash records exit status `crashed` and a
  crash-log artifact reference that resolves to a stored, hash-verified
  file.
- [x] 6.6 Test: a run with exit status `succeeded` and correctness result
  `failed` round-trips both fields independently (neither overwrites the
  other).
- [x] 6.7 Test: two run rows with different repetition numbers for the same
  logical retry both persist independently and remain individually
  queryable (replaces the old `runs_repetition_uniqueness.rs` duplicate-
  rejection test with one for the new "duplicate run ID is rejected, but
  repetition numbers are not database-unique" behavior).
- [x] 6.8 Test: environment-variable allowlist JSON preserves an explicit
  "unset" vs. "empty string" distinction across a round trip.
- [x] 6.9 Test: inserting a run with an invalid exit status or correctness
  value is rejected by the database `CHECK` constraint.
- [x] 6.10 Test: inserting a run with a negative token count or negative
  frequency is rejected by the database `CHECK` constraint.
- [x] 6.11 Test: constructing a run or artifact with a malformed SHA-256
  (wrong length, uppercase, non-hex) is rejected at the application boundary
  before any query runs.
- [x] 6.12 Test: inserting a run referencing a nonexistent artifact ID is
  rejected by the foreign-key constraint (with `PRAGMA foreign_keys = ON`
  confirmed active on the test connection).
- [x] 6.13 Test: writing the same artifact content twice reuses the existing
  `artifacts` row (dedup on `(sha256, size_bytes)`) rather than creating a
  second row or a second file.
- [x] 6.14 Test: a raw insert that violates a foreign key fails with SQLite's
  foreign-key-constraint error specifically (not silently succeeding),
  confirming enforcement is actually active end-to-end.
- [x] 6.15 Test: concurrent reads succeed while a short write transaction is
  in progress (spawn a reader task and a writer task against the same pool
  and confirm the reader completes without error under WAL mode).
- [x] 6.16 Test: restarting the application (closing and reopening a
  `SqlitePool` against the same on-disk file) reads back previously stored
  run and artifact data unchanged.
- [x] 6.17 Test: running migrations against a completely empty (freshly
  created, zero-byte) SQLite file succeeds and results in the expected
  schema.
- [x] 6.18 Update `tests/http_health.rs` to use the SQLite-backed
  `migrated_pool`; keep both the healthy and closed-pool failure scenarios.
  Verify both tests pass.
- [x] 6.19 Delete `tests/testcontainers_smoke.rs` and the old
  `tests/run_metrics_phases.rs`/`tests/validations_independent_of_run_status.rs`
  (superseded by 6.5/6.6 against the new schema). Verify `cargo test` no
  longer references `testcontainers`.

## 7. Verification

- [x] 7.1 Run `cargo fmt` and confirm no diff remains.
- [x] 7.2 Run `cargo test` and confirm the full suite passes with no Docker
  daemon running.
- [x] 7.3 Run `cargo clippy --all-targets -- -D warnings` and confirm zero
  warnings.
- [x] 7.4 Apply all migrations to a brand-new empty SQLite file with the
  `sqlx-cli` (`cargo sqlx migrate run`) and inspect the resulting schema and
  indexes with `sqlite3 <file> .schema` and `.indexes runs`/`.indexes
  artifacts`.
- [x] 7.5 Perform one end-to-end example insertion and retrieval by hand
  (e.g. a small `cargo run --example` or a manual `cargo run` + `sqlite3`
  query) covering a run with an attached artifact.
- [x] 7.6 Confirm the application restarts successfully and reads
  previously stored data: run the service once to write data, stop it,
  start it again against the same `data/benchmarks.sqlite3`, and confirm a
  query against previously written rows succeeds.
- [x] 7.7 Regenerate the `.sqlx` offline query cache (`cargo sqlx prepare`)
  against the new SQLite schema and commit it.
- [x] 7.8 Search the repository for remaining PostgreSQL references
  (`rg -i "postgres|pgpool|jsonb|timestamptz"`) and confirm every hit is
  either removed or explicitly documented as an intentionally retained
  remnant in the final report.
