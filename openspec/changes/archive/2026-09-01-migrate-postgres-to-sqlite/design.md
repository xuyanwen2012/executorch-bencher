## Context

The current codebase (from `2026-08-31-init-benchmark-db-architecture`) is a
service skeleton only: `src/{main,config,db,http}.rs`, nine PostgreSQL
migrations (campaigns, device/source snapshots, artifacts, run
configurations, a `run_status` enum, runs, metrics, run_metrics, run_telemetry,
validations), a `docker-compose.yml` Postgres service, and
`testcontainers-rs`-based integration tests. No ingestion endpoints exist yet
and no data has ever been written against this schema — see `proposal.md` -
Why. This design covers the concrete SQLite schema, pragma handling,
migration replacement, artifact-storage implementation, and module layout
needed to satisfy the updated `specs/benchmark-schema` and
`specs/ingestion-service` deltas.

## Goals / Non-Goals

**Goals:**
- Replace `sqlx::PgPool`/`postgres` with `sqlx::SqlitePool`/`sqlite`
  end-to-end, with every pooled connection carrying the required pragmas.
- Land a single SQLite-compatible initial migration set for `runs`,
  `artifacts`, and `schema_metadata`, replacing the nine PostgreSQL
  migrations outright.
- Implement a content-addressed local filesystem artifact store with
  temp-write → hash-verify → atomic-rename semantics, behind a small
  repository-style module so `http`/future ingestion handlers never touch
  `std::fs` or raw SQL directly.
- Make `cargo test` fully hermetic with no Docker/Postgres dependency:
  temporary SQLite files (or `:memory:` where appropriate) and temporary
  artifact-root directories per test.
- Keep `SQLX_OFFLINE=true` builds working via a regenerated `.sqlx` cache.

**Non-Goals:**
- No ingestion HTTP endpoints beyond the existing `/health` check - this
  change only replaces the data layer under the existing skeleton.
- No object storage, Docker, or any external service of any kind.
- No campaigns, run-configuration comparability fingerprints, metrics
  catalog, or telemetry time series - see the REMOVED requirements in
  `specs/benchmark-schema/spec.md`.

## Decisions

### Schema: three tables (`runs`, `artifacts`, `schema_metadata`)
`runs` inlines what were previously `device_snapshots`, `source_snapshots`,
and `run_configurations` as columns; `metrics`/`run_metrics` collapse into
two fixed columns (`prefill_tokens_per_sec`, `decode_tokens_per_sec`);
`run_telemetry` collapses into `initial_temperature_celsius` and
`max_temperature_celsius`; `validations` collapses into
`correctness_result`. `artifacts` is carried over with a `storage_path`
relative to the configured artifact root instead of an opaque `storage_uri`.
`schema_metadata` is a new single-row table recording the applied schema
version (see "schema_metadata is a single row, updated in place" below).
- **Why**: Matches the MVP data model in the proposal exactly; SQLite has no
  advantage from normalizing point-in-time snapshot data into separate
  tables here, and the prior normalized design's main payoff (campaign
  grouping, comparability fingerprints, extensible metric catalog) is
  explicitly deferred.
- **Alternative considered**: keep the normalized multi-table shape and just
  swap the SQL dialect - rejected because the proposal explicitly calls for
  schema simplification, and the normalized shape's benefits (dedup via
  `source_snapshots` reuse, generic metric catalog) are the parts requested
  to be dropped for now.

### Run IDs: UUIDv7 generated in Rust, stored as `TEXT`
SQLite has no native UUID type. Run IDs (and artifact IDs) are generated with
the `uuid` crate's `Uuid::now_v7()` in the application and stored as their
canonical 36-character hyphenated string form in `TEXT` columns.
- **Why**: Matches the prior UUIDv7 decision (time-ordered inserts) while
  moving generation out of the database, since SQLite has no
  `gen_random_uuid()` equivalent and the proposal explicitly calls for
  Rust-side generation.
- **Alternative considered**: SQLite `INTEGER PRIMARY KEY` (rowid) autoincrement
  ids - simpler and marginally faster, but loses the ability to generate an
  id client-side before insertion (useful for idempotent retries and
  artifact content-addressing symmetry) and abandons the existing UUIDv7
  convention for no strong reason.

### Timestamps: UTC RFC 3339 text
All timestamp columns (`started_at`, `finished_at`, artifact `created_at`,
`schema_metadata` timestamps) are stored as `TEXT` in RFC 3339 with a `Z`
suffix (e.g. `2026-09-01T12:34:56.789Z`), produced by `chrono`'s
`DateTime<Utc>::to_rfc3339_opts`. Rust types use `chrono::DateTime<Utc>` via
SQLx's `chrono` feature (which supports SQLite) and (de)serialize to the same
representation in API payloads and tests.
- **Why**: RFC 3339 text sorts correctly as a string for the common case
  (fixed-width fractional seconds, `Z` UTC suffix), is human-readable in
  `sqlite3` CLI inspection, and matches the proposal's preference. Unix
  milliseconds were the documented alternative but offer no advantage once a
  fixed-width text format is chosen, and are harder to eyeball while
  debugging.
- **Alternative considered**: Unix epoch milliseconds as `INTEGER` - slightly
  cheaper to compare/sort and avoids text-format edge cases, but is opaque
  when inspecting the database by hand; rejected since this project's
  primary consumer during development is a human running `sqlite3
  data/benchmarks.sqlite3`.

### JSON columns validated at the application boundary before insertion
Command-line argument arrays, input parameters, and the environment-variable
allowlist capture are stored as `TEXT` columns holding canonical
(`serde_json::to_string` on a `serde_json::Value` parsed and re-serialized,
so key order and whitespace are consistent) JSON, with
`CHECK (json_valid(column))` as a defense-in-depth database constraint plus
explicit `serde_json::from_str` validation in the repository layer before
every insert.
- **Why**: SQLite's `json_valid()` is a cheap, built-in guard against
  malformed JSON reaching storage, matching the proposal's "validate JSON
  before insertion" requirement without needing a `jsonb` type SQLite
  doesn't have. Validating again in the application layer produces a
  friendlier error before the query even runs.
- **Alternative considered**: rely solely on the `CHECK` constraint - simpler,
  but produces an opaque SQLite constraint-violation error instead of a
  typed application error, and doesn't allow validating structure (not just
  "is this valid JSON") such as the environment-allowlist's expected shape.

The command-argument array is additionally validated for JSON's own type
shape implied by its Rust representation (it must deserialize as
`Vec<String>`, not just any array); input parameters and the
environment-variable allowlist capture are validated only for
well-formedness (plus, for the allowlist, being a top-level JSON object),
since their internal structure is otherwise caller-defined.

### Performance configuration: three fixed clock columns, not a frequency map
Performance configuration is narrowed to exactly three named, pinned clocks
- GPU clock, MIF (memory-interface) clock, and INT (interconnect) clock -
stored as plain `INTEGER NOT NULL` columns in MHz with `CHECK (... > 0)` and
documented defaults (`gpu_clock_mhz` 980, `mif_clock_mhz` 5333,
`int_clock_mhz` 934), replacing the originally planned open-ended
CPU-cluster-frequency JSON map plus separate GPU/memory-interface columns.
- **Why**: The target hardware's performance configuration is exactly these
  three clocks; an open-ended per-cluster JSON map added generality (varying
  cluster names/counts across Android devices) that isn't needed for the
  actual device this MVP targets, and plain scalar columns with `CHECK`
  constraints are simpler to validate, index, and query than a JSON map. No
  JSON validation helper is needed for performance configuration as a
  result - each clock is a scalar `INTEGER` enforced entirely by the
  database `CHECK`.
- **Alternative considered**: keep the per-cluster JSON map for CPU
  frequencies alongside two more scalar columns (as originally planned) -
  more general across heterogeneous Android CPU topologies, but rejected
  once the actual requirement narrowed to three fixed, named clocks with no
  CPU-cluster concept at all.
- **Units**: MHz, not Hz - unlike the rest of this design's "prefer Hz"
  default, these three values are supplied and reported by tooling in MHz
  (e.g. `980`, not `980000000`); using MHz directly avoids a unit
  conversion at the application boundary and matches how the values are
  read from the device.

### schema_metadata is a single row, updated in place
`schema_metadata` holds one row (`id INTEGER PRIMARY KEY CHECK (id = 1)`)
that each schema-version-bumping migration overwrites with the new version,
rather than appending a new row per migration.
- **Why**: SQLx's own `_sqlx_migrations` table (created automatically by
  `sqlx::migrate!()`) already records full migration history; a
  `schema_metadata` append-log would just duplicate it. Only "what version is
  this database at right now" is operationally useful to query without
  SQLx-specific knowledge.
- **Alternative considered**: append-only log, one row per applied migration
  - gives a human-inspectable history without needing to know about
  `_sqlx_migrations`, but duplicates information SQLx already tracks for no
  clear benefit at this scale; rejected as unnecessary.

### No database-level duplicate-submission protection beyond the run ID primary key
The MVP schema does not reintroduce a uniqueness constraint keyed on
device/commit/repetition (or similar) to catch a caller submitting the "same"
repetition twice; only a literal run ID collision is rejected via the
primary key.
- **Why**: The backend is the only writer, and repetition numbers are
  caller-assigned metadata rather than a database-enforced key in the MVP
  scope (see the corresponding MODIFIED requirement in
  `specs/benchmark-schema/spec.md`). Revisit if duplicate submissions prove
  to be a real operational problem once the ingestion endpoints exist.
- **Alternative considered**: `UNIQUE (device_serial, git_commit_sha,
  repetition)` - would catch accidental duplicate submissions at the
  database level, but invents a comparability key the proposal didn't ask
  for and couples run uniqueness to a specific choice of "what makes two
  runs the same attempt" that may not hold once retries or multi-device
  fan-out are added; deferred.

### Exit status and correctness result as `CHECK`-constrained `TEXT`
Both columns are plain `TEXT NOT NULL` with a `CHECK (col IN (...))`
constraint enumerating the fixed vocabulary from the proposal
(`succeeded`/`crashed`/`timed_out`/`cancelled`/`infrastructure_error` for
exit status; `passed`/`failed`/`not_checked`/`validator_error` for
correctness), replacing the PostgreSQL `run_status` `ENUM` type.
- **Why**: SQLite has no native enum type; a `CHECK` constraint gives
  equivalent database-level enforcement with a plain-text representation
  that's trivial to inspect and doesn't require an `ALTER TYPE`-equivalent
  migration ceremony to extend later.
- **Alternative considered**: enforce the vocabulary only in the application
  layer (Rust enum + `sqlx` `Type` mapping), no database `CHECK` - rejected
  because the proposal explicitly asks for `CHECK` constraints on status
  values, and a database-level guard protects against any future write path
  that bypasses the Rust type (e.g. an ad hoc `sqlite3` shell fix).

### SHA-256 validated as 64-character lowercase hex at the application boundary
A newtype (e.g. `Sha256Hex(String)`) validates the 64-character
lowercase-hex shape at construction (via `TryFrom<String>`), used for
executable/model/prompt hashes on `runs` and the `sha256` column on
`artifacts`. A `CHECK (length(sha256) = 64)` constraint backs it in the
schema as a coarse database-level guard.
- **Why**: Matches the proposal's explicit validation requirement; SQLite has
  no regex `CHECK` support without a loaded extension, so the database-level
  constraint only checks length while the Rust newtype does the full
  hex-character validation, giving the strong guarantee at the boundary that
  actually accepts external input.
- **Alternative considered**: validate only in Rust, no database `CHECK` -
  simpler schema, but drops a cheap, free length guard against any
  non-application write path; rejected since the length check costs nothing.

### Content-addressed artifact storage: temp-write, hash, atomic rename
A new `artifact_store` module writes incoming bytes to
`<artifact_root>/tmp/<random>`, computes SHA-256 while streaming (via the
`sha2` crate), then `std::fs::rename`s the temp file to
`<artifact_root>/sha256/<first-2-hex-chars>/<full-64-hex-hash>` (same
filesystem, so rename is atomic), and only then inserts (or, on a
`(sha256, size_bytes)` conflict, reuses) the `artifacts` row inside the same
function so no artifact ever gets inserted after a failed or partial file
write. Public API responses expose an artifact ID, never a filesystem path.
- **Why**: Directly implements the proposal's five-step "artifact writes
  should be safe" sequence and its "no raw filesystem paths in API
  responses" requirement.
- **Alternative considered**: write directly to the final content-addressed
  path and skip the temp-file/rename step - simpler, but risks a partially
  written file being treated as complete if the process is interrupted
  mid-write; rejected per the proposal's explicit safety sequence.

### SQLite connection configuration: pragmas applied per-connection via `SqliteConnectOptions`
`journal_mode = WAL` is persisted in the database file after being set once,
but `foreign_keys`, `busy_timeout`, and `synchronous` are per-connection
session settings in SQLite and must be reapplied on every new connection the
pool opens - not just at startup. This is done via
`SqliteConnectOptions::foreign_keys(true).busy_timeout(Duration::from_millis(5000)).synchronous(SqliteSynchronous::Full).journal_mode(SqliteJournalMode::Wal)`
passed to `SqlitePoolOptions::connect_with`, which SQLx applies to every
connection the pool creates (not only the first), rather than running raw
`PRAGMA` statements once after connecting.
- **Why**: A connection pool may open new connections after startup (e.g. to
  serve concurrent requests beyond the first), and a pragma set via a
  one-time `PRAGMA` statement immediately after the initial connection would
  not apply to those later connections - directly matching the "verify
  which pragmas are persistent vs. per-connection" instruction in the
  proposal and the new ingestion-service requirement.
- **Alternative considered**: run `PRAGMA` statements manually in an
  `after_connect` pool hook - functionally equivalent, but
  `SqliteConnectOptions`'s typed builder is less error-prone (compile-time
  checked values, no hand-written SQL string) and is SQLx's documented
  mechanism for this exact case.

### Migrations: single fresh SQLite initial migration, old PostgreSQL migrations deleted
The nine existing PostgreSQL migration file pairs are deleted outright and
replaced with one new `..._init_sqlite_schema.up.sql`/`.down.sql` pair
(or a small number split by table, following the existing project
convention of one migration per table) creating `runs`, `artifacts`, and
`schema_metadata`.
- **Why**: Per proposal's "Migration handling": the project has shipped no
  code and holds no data, so this qualifies as "only an unused initial
  schema" - a clean replacement is explicitly preferred over a
  data-preserving migration path in that case.
- **Alternative considered**: leave the old PostgreSQL migration files in
  `migrations/` alongside new SQLite ones - rejected; they reference a
  different SQL dialect entirely (they are not "old data to migrate", they
  are dead code that would never run against SQLite) and keeping them would
  be actively misleading.

### Test infrastructure: `tempfile` crate, no `testcontainers`
Integration tests use `tempfile::tempdir()` to create a fresh SQLite file
path and a fresh artifact-root directory per test, run migrations against
that fresh file, and drop the temp directory (auto-cleanup) at the end of
the test. `testcontainers`/`testcontainers-modules` are removed from
`[dev-dependencies]`.
- **Why**: SQLite's whole value proposition here is not needing an external
  process; a temp file is faster to provision than a container and needs no
  Docker daemon, matching the proposal's "no Docker, no externally installed
  database" test requirement.
- **Alternative considered**: an in-memory SQLite database
  (`sqlite::memory:`) for tests - faster still, but a single in-memory
  database is tied to one connection unless using SQLite's shared-cache mode
  (extra configuration complexity), and doesn't exercise the same
  file-open/WAL-file code path production runs through; a temp file is used
  for the primary test suite, with `:memory:` reserved for any narrowly
  scoped unit test that doesn't care about that distinction.

### Module layout: unchanged `config`/`db`/`http`, new `artifact_store`
`config.rs`, `db.rs`, `http.rs`, `main.rs` keep their existing
responsibilities (env config, pool + migration, router/handlers, bootstrap)
with their internals swapped from Postgres to SQLite types. A new
`src/artifact_store.rs` owns the temp-write/hash/rename/dedup logic, called
from wherever runs are ingested; `src/lib.rs` gains `pub mod artifact_store;`.
- **Why**: Keeps the change a data-layer swap rather than a restructuring;
  the existing module boundaries already separate concerns cleanly enough
  that only their internals need to change, consistent with "reuse the
  current backend structure where reasonable."

## Risks / Trade-offs

- [Risk] SQLite's single-writer model means concurrent write-heavy usage
  (multiple runners submitting simultaneously) could see `busy_timeout`
  contention under load. -> Mitigation: `busy_timeout = 5000` gives writers
  headroom to queue rather than fail immediately; WAL mode allows readers to
  proceed concurrently with a writer. This is an accepted trade-off of the
  proposal's explicit choice of "backend process is the only SQLite opener,
  everyone else goes through the API" - the API serializes writes anyway.
- [Risk] Dropping the `run_configurations`-keyed duplicate-repetition
  uniqueness constraint means the database no longer rejects two runs that a
  caller intends as "the same repetition of the same conditions" (only
  literal run-ID collisions are rejected). -> Mitigation: this is an
  intentional MVP scope cut (see the corresponding REMOVED/MODIFIED
  requirement in `specs/benchmark-schema/spec.md`); revisit if duplicate
  submissions become a real operational problem.
- [Risk] Collapsing device/source/config snapshots into columns on `runs`
  means any future need to reintroduce cross-run grouping (e.g. campaigns)
  requires backfilling a new entity from existing `runs` rows rather than
  having one ready. -> Mitigation: accepted; the proposal explicitly defers
  this, and a backfill from `runs` columns is straightforward since all the
  source data is already present per-row.
- [Risk] Deleting the old PostgreSQL migrations changes `migrations/`
  filenames/checksums that SQLx's migrator tracks. -> Mitigation: no
  deployed database exists to have applied the old migrations, so there is
  no `_sqlx_migrations` history to conflict with; this is safe only because
  the project has never shipped, per proposal's migration-handling guidance.

## PostgreSQL remnants intentionally retained

None expected. `docker-compose.yml`'s Postgres service, the `postgres`
SQLx feature, `testcontainers`/`testcontainers-modules`, and all
PostgreSQL-dialect SQL are removed as part of this change. If implementation
surfaces a remnant that can't be cleanly removed, it must be called out
explicitly in the final report with a reason, per the proposal's
verification checklist.
