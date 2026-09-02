## 1. Project dependencies and local environment

- [x] 1.1 Add `sqlx` (features: `runtime-tokio`, `postgres`, `uuid`, `chrono` or `time`, `macros`, `migrate`), `tokio` (features: `rt-multi-thread`, `macros`), `axum`, `uuid` (feature: `v7`, `serde`), `serde`/`serde_json` to `Cargo.toml`, and verify `cargo check` succeeds
- [x] 1.2 Add `docker-compose.yml` at the crate root defining a Postgres service for local development, and verify `docker compose up -d` starts a reachable Postgres instance
- [x] 1.3 Install `sqlx-cli` and document (in a short `README.md` section or comment) the `cargo sqlx prepare` workflow for regenerating the offline query cache against the docker-compose database, and verify running it produces a `.sqlx/` directory
- [x] 1.4 Add `testcontainers` and a Postgres testcontainers module as dev-dependencies in `Cargo.toml`, and verify a minimal smoke test can start and connect to a containerized Postgres instance

## 2. Migrations - core entities

- [x] 2.1 Create migration for `benchmark_campaigns` table (`created_by` as a plain `uuid NOT NULL` with no foreign key, since no `users` table exists yet) and verify it applies cleanly via `sqlx migrate run` against a `testcontainers-rs`-provisioned database
- [x] 2.2 Create migration for `device_snapshots` table (immutable snapshot fields: device id, board model/revision, serial number, soc model, memory, bsp/kernel/driver/firmware versions as columns plus `jsonb` for uncommon fields, `captured_at`) and verify it applies cleanly
- [x] 2.3 Create migration for `source_snapshots` table (repository, branch, commit sha, commit time, dirty flag, dirty diff artifact reference, build id/options, unique constraint on `(repository, commit_sha, dirty, build_id)`) and verify it applies cleanly
- [x] 2.4 Create migration for `artifacts` table (kind, original name, storage uri, size, sha256, optional md5, media type, unique constraint on `(sha256, size_bytes)`) and verify it applies cleanly, then add the deferred foreign key from `source_snapshots.dirty_diff_artifact_id` to `artifacts(id)`

## 3. Migrations - configurations and runs

- [x] 3.1 Create migration for `run_configurations` table (campaign fk, source snapshot fk, input artifact fk, model name/version, command template, parameters/environment `jsonb`, `configuration_hash` column with an index) and verify it applies cleanly
- [x] 3.2 Create migration defining the `run_status` Postgres ENUM type (`queued`, `running`, `succeeded`, `crashed`, `timed_out`, `cancelled`, `invalid_output`, `infrastructure_error`) and verify it applies cleanly
- [x] 3.3 Create migration for `runs` table (configuration fk, device snapshot fk, repetition, status, timestamps, exact command, working directory, process id, exit code/signal, initial/final temperature, stdout/stderr/output/crash artifact fks, error message, collector version, `extra` jsonb, unique constraint on `(configuration_id, device_snapshot_id, repetition)`) and verify it applies cleanly
- [x] 3.4 Write a migration-level integration test, using `testcontainers-rs` to provision the database, that inserts a campaign, snapshot, configuration, and two run repetitions (application-generated UUIDv7 primary keys), and verify a duplicate third insert with a repeated repetition number is rejected by the unique constraint

## 4. Migrations - metrics, telemetry, validations

- [x] 4.1 Create migration for `metrics` catalog table (name, unit, description, `lower_is_better`, unique on name; table ships with no seed rows) and verify it applies cleanly
- [x] 4.2 Create migration for `run_metrics` table (run fk, metric fk, phase, value, sample count, metadata jsonb, composite primary key `(run_id, metric_id, phase)`) and verify it applies cleanly, and add a `testcontainers-rs`-backed test inserting the same metric for two different phases of one run
- [x] 4.3 Create migration for `run_telemetry` table (run fk, observed_at, sensor, value, unit, composite primary key `(run_id, observed_at, sensor)`) and verify it applies cleanly
- [x] 4.4 Create migration for `validations` table (run fk, validator name/version, status, expected artifact fk, score, details jsonb, created_at) and verify it applies cleanly, and add a `testcontainers-rs`-backed test recording a validation failure against a run whose own `status` is `succeeded`, confirming the run's `status` column is unaffected

## 5. Service skeleton

- [x] 5.1 Create `src/config.rs` loading database connection settings from environment variables, and verify a unit test covers both the present and missing/invalid configuration cases
- [x] 5.2 Create `src/db.rs` with a function that builds a Postgres connection pool and applies pending migrations via `sqlx::migrate!()`, using `query!`/`query_as!` macros for any queries it needs, and verify the service exits with a clear error when pointed at an unreachable database (per `specs/ingestion-service` "Database is unreachable at startup")
- [x] 5.3 Create `src/http.rs` with an Axum router exposing a `/health` route that checks the database pool (e.g. `SELECT 1`) and returns success/failure accordingly, and verify both the healthy and DB-down scenarios with a `testcontainers-rs`-backed integration test
- [x] 5.4 Rewrite `src/main.rs` to wire config -> db pool + migrate -> router -> `axum::serve`, and verify `cargo run` starts the service against the docker-compose Postgres instance and `GET /health` returns success
- [x] 5.5 Run `cargo sqlx prepare` against the docker-compose database to regenerate the `.sqlx` offline query cache, commit it, and verify `cargo check` succeeds with `SQLX_OFFLINE=true` and no database running

## 6. Verification

- [x] 6.1 Run the full migration set from empty against a fresh Postgres instance (docker-compose or testcontainers) and verify no errors and that `\dt`/`\d` shows all expected tables, the `run_status` enum, and constraints
- [x] 6.2 Run `cargo test` and verify all schema and service integration tests pass using `testcontainers-rs`-provisioned databases
- [x] 6.3 Run `openspec validate init-benchmark-db-architecture --strict` and verify it passes
