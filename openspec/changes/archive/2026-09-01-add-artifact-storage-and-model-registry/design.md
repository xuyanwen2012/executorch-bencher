## Context

The current codebase (`src/{main,config,db,http,artifact_store,runs,domain}.rs`,
three migrations) is a working SQLite-backed skeleton: `artifacts`
(`stdout`/`stderr`/`crash_log` only, non-streaming `store_artifact(&[u8])`
writing under `<artifact_root>/tmp/`), `runs` (with `model_sha256` and
`prompt_sha256` as bare hashes, `stdout/stderr/crash_artifact_id` FKs to
`artifacts`), and `schema_metadata`. `src/http.rs` exposes only `/health`.
`Config` reads `DATABASE_URL` and `ARTIFACT_ROOT` from the environment; there
is no model root, temp/trash directory, upload-size limit, or preview-length
configuration. See `proposal.md` - Why for the gap this closes, and
`specs/artifact-storage`, `specs/benchmark-schema`,
`specs/ingestion-service` for the resulting behavioral contract.

## Goals / Non-Goals

**Goals:**
- Rewrite `artifact_store` for streaming ingestion (bounded memory, hash
  computed incrementally) using a `temporary/` directory that is a sibling
  of `artifacts/`, not nested inside it.
- Add Zstandard compression for log-shaped artifact kinds, with SHA-256
  identity always computed over uncompressed bytes.
- Add a `model_registry` module implementing external-mode registration and
  cached-checksum verification, plus a `ModelStorage` abstraction that
  external mode implements now and managed mode can implement later without
  changing callers.
- Add `model_assets` table, and `model_asset_id`/`input_artifact_id`/
  `output_artifact_id` to `runs`.
- Add artifact and model HTTP endpoints, streaming content and never leaking
  server filesystem paths.
- Add a read-only integrity-report maintenance operation.

**Non-Goals:**
- Managed-mode model storage's HTTP registration/copy path is not
  implemented in this change (see "Managed mode: abstraction now, API
  later" below) — only its schema shape and storage-trait contract are.
- No automatic garbage collection or trash-emptying job; the integrity
  report is read-only, matching the proposal's explicit deferral.
- No new authentication system; `/health` and the new routes remain
  unauthenticated, consistent with the existing skeleton having none.
- No changes to `runs`' device-state, environment-allowlist, or performance-
  configuration fields beyond what's listed in the proposal.

## Decisions

### Filesystem layout: one configurable data root, four named subpaths
`Config` gains `data_root: PathBuf` plus four fields resolved beneath it by
default but independently overridable: `artifact_root` (`<data_root>/artifacts`),
`model_root` (`<data_root>/models`), `temporary_dir` (`<data_root>/temporary`),
`trash_dir` (`<data_root>/trash`). Each is read from its own environment
variable (`ARTIFACT_ROOT`, `MODEL_ROOT`, `TEMPORARY_DIR`, `TRASH_DIR`) with a
default derived from `DATA_ROOT` when unset, preserving today's
`ARTIFACT_ROOT`-only override behavior for the artifact root specifically.
Startup creates all four directories (`create_dir_all`) and fails clearly,
naming the specific root, if any cannot be created or is not writable —
no fallback to a different directory.
- **Why**: The proposal's filesystem layout is a suggested default shape, but
  the "Configuration" section separately asks for each root to be
  independently configurable. Deriving defaults from one `data_root` keeps
  the common case (one `DATA_ROOT=./benchmark-data`) simple while still
  satisfying "explicit configuration for... artifact root, model root,
  temporary directory, trash directory" as independently settable.
- **Alternative considered**: keep only `ARTIFACT_ROOT` and hang `models/`,
  `temporary/`, `trash/` off it implicitly — simpler, but conflates "root
  that holds content-addressed artifact blobs" with "root that holds
  everything," and forecloses deploying the (typically far larger) model
  root on separate storage later.

### `temporary/` moves out from under `artifacts/`
Today's `store_artifact` writes to `<artifact_root>/tmp/`. The rewrite uses
the new top-level `temporary_dir` instead, so a directory listing of
`artifacts/` (or `models/`) contains only `sha256/`-addressed content, and
the integrity report's "orphaned temp file" scan doesn't need to special-case
excluding a `tmp/` subdirectory from its "files without database rows" pass.
- **Why**: Matches the proposal's explicit filesystem layout and simplifies
  the integrity report's file-walk logic.
- **Alternative considered**: keep `tmp/` nested per-root (one under
  `artifacts/`, one under `models/`) — avoids a shared temp directory across
  artifact and model ingestion, but the proposal's layout specifies a single
  top-level `temporary/`, and a shared directory is fine since every
  temp-file name is already a random UUID with no kind-based collision risk.

### Streaming ingestion: `tokio::io::AsyncWrite` + incremental `Sha256`
`store_artifact` becomes `async fn store_artifact(pool, roots, kind,
original_name, media_type, mut reader: impl AsyncRead + Unpin) ->
Result<StoredArtifact, _>`, reading in fixed-size chunks (e.g. 64 KiB),
writing each chunk to the temp file, and feeding it to a running
`sha2::Sha256` hasher — mirroring the "flow bytes in, hash and write
concurrently, never buffer the whole thing" sequence in the proposal. The
existing byte-slice call sites (tests, `store_artifact(..., b"...")`) switch
to wrapping the slice in a reader; no behavior changes for small inputs.
Concurrent identical uploads are handled exactly as today: both writers reach
the same final content-addressed path, the loser of the `rename` race (or
the one that observes the destination already exists) discards its temp file
and both resolve to one `find_existing`-or-inserted row, with the existing
unique-violation-retry fallback on the insert race.
- **Why**: Directly implements the proposal's ten-step ingestion sequence and
  the "read once, don't load the complete file into memory" requirement
  without pulling in a new HTTP framework or hashing library.
- **Alternative considered**: `std::fs::File` + blocking `std::io::copy` on a
  `spawn_blocking` task — equally memory-bounded, but the Axum handler
  receiving a streamed multipart/body upload already produces an async byte
  stream, so an async writer avoids an extra thread-pool hop per chunk.

### Compression: `zstd` crate, applied only to log-shaped kinds, identity over plaintext
A `compress: bool` decision is made from `ArtifactKind` alone
(`stdout`/`stderr`/`crash_log`/`logcat`/`correctness_report` → `true`;
`prompt`/`output` → `false`). When compressing, the incoming stream is hashed
*before* compression (a `TeeReader`-style wrapper feeds both the hasher and a
`zstd::stream::write::Encoder` wrapping the temp file); the stored file is
the compressed bytes, `sha256`/`size_bytes` describe the original plaintext,
and `compression = 'zstd'` is recorded. Retrieval streams the file through
`zstd::stream::read::Decoder` when `compression = 'zstd'`.
- **Why**: The proposal explicitly asks for uncompressed-content identity
  "unless the existing codebase has a compelling reason to standardize on
  stored-byte identity" — this codebase has none (today's uncompressed
  artifacts already hash the original content), so keeping SHA-256 tied to
  what a user would recognize as "the log" (not an implementation detail of
  how it happens to be stored) is the natural continuation, and it lets
  future clients verify a downloaded, transparently-decompressed log against
  the recorded hash directly.
- **Alternative considered**: hash the compressed bytes (stored-byte
  identity) — simpler (one hash pass over what's actually written, no tee),
  but breaks the invariant that an artifact's SHA-256 is "the hash of its
  content" from a caller's point of view, and would make the existing
  `UNIQUE (sha256, size_bytes)` dedup constraint compression-level-dependent
  (re-compressing identical content at a different level would register as
  a new artifact).

### `model_assets` and `ModelStorage` trait: external implemented, managed abstracted
`model_registry.rs` defines a `ModelStorage` trait (`register`, `verify`,
`resolve_content_path`) with an `ExternalModelStorage` implementation doing
exactly the proposal's five registration steps and four pre-run-verification
steps (stat → compare cached size+mtime → reuse-or-rehash → mark
availability), plus a `verify_full` operation that always rehashes and
updates `last_verified_at`. `storage_mode` is stored per-row so a
`ManagedModelStorage` (writing to `models/sha256/<prefix>/<sha256>` with the
same dedup-by-checksum shape as artifact storage) can be added later behind
the same trait and the same `model_assets` schema, with no migration needed
when it lands — only new registration code and a new HTTP request variant.
This change does not implement `ManagedModelStorage` or a managed-mode
registration endpoint.
- **Why**: The proposal explicitly permits deferring managed mode "if
  implementing both modes now adds substantial complexity," provided the
  deferral is reported and the abstraction is clean; external mode is the
  documented default for the current 14 GB models, so it carries all of this
  change's actual operational value, while managed mode's main use case
  (smaller shared models copied once) has no immediate driver yet.
- **Alternative considered**: implement managed mode's copy-and-dedup path
  now too, reusing `store_artifact`'s temp-write/rename/dedup logic against
  the model root — was scoped out only because doing register-time
  streaming *and* a second content root *and* a second HTTP surface in one
  change meaningfully increases review surface for a code path with no
  current caller; the trait boundary means adding it later touches
  `model_registry.rs` and one new route, not the schema or `runs`.

### External-model change detection: cached `(size_bytes, file_modified_at)` compare
Before a run uses a registered external model, the registry stats the file
and compares `(len, mtime)` against the stored `size_bytes`/
`file_modified_at`. Unchanged → reuse `sha256`/mark `available = 1` without
reading file content. Either differs → stream-rehash, update
`sha256`/`size_bytes`/`file_modified_at`/`last_verified_at` on that same row
(same SHA-256-keyed identity is not assumed stable — if the rehash produces
a SHA-256 that collides with a *different* already-registered row, that
existing row is reused and this one is marked unavailable, mirroring
artifact dedup). Missing file → `available = 0`, no rehash. `verify_full`
always rehashes regardless of cached size/mtime and always updates
`last_verified_at`.
- **Why**: Matches the proposal's explicit "do not recalculate the checksum
  of an unchanged 14 GB model before every run" requirement while still
  detecting drift; mtime+size is the standard cheap-staleness heuristic
  (same one `make`/rsync use) and avoids a content hash on the hot path.
- **Alternative considered**: hash on every run regardless of staleness —
  simplest and always correct, but a 14 GB SHA-256 pass per run is exactly
  what the proposal calls out as unacceptable.

### `runs.model_asset_id` is `NOT NULL`; `input_artifact_id`/`output_artifact_id` are nullable
Every run exercises exactly one model, so `model_asset_id TEXT NOT NULL
REFERENCES model_assets(id)` replaces `model_sha256`. `input_artifact_id` and
`output_artifact_id` stay nullable `TEXT REFERENCES artifacts(id)`, matching
`stdout_artifact_id`/`stderr_artifact_id`/`crash_artifact_id`, since a run
may be recorded before its output is captured (or, for older-style callers,
without ever capturing the exact prompt as an artifact — `prompt_sha256`
remains as the always-present scalar hash for that case).
- **Why**: `model_asset_id NOT NULL` matches the domain reality (a run always
  ran some specific model) and gives the strongest FK guarantee; the other
  two follow the existing nullable-artifact-FK pattern already established
  for stdout/stderr/crash.
- **Alternative considered**: make `model_asset_id` nullable too, for
  symmetry with the artifact FKs — rejected because "which model produced
  this result" is core reproducibility data the schema already treats as
  mandatory today (`model_sha256 TEXT NOT NULL`); loosening it would be a
  regression, not a simplification.

### Migration split: alter `artifacts`, create `model_assets`, alter `runs`
SQLite's limited `ALTER TABLE` (no `ADD CONSTRAINT`, no dropping a `CHECK`)
means widening `artifacts.kind`'s `CHECK` and adding `compression` requires
the project's already-established pattern of a new migration that creates a
replacement table, copies data, drops the old one, and renames — not an
in-place `ALTER TABLE ... ADD COLUMN` for the `CHECK`-constrained parts
(the new nullable `compression` column with a `DEFAULT 'none'` *can* be a
plain `ADD COLUMN`; the widened `kind` `CHECK` cannot). Three migrations
follow the existing one-migration-per-table convention: (1) rebuild
`artifacts` with the widened `kind` `CHECK` and new `compression` column,
(2) create `model_assets`, (3) rebuild `runs` adding `model_asset_id`/
`input_artifact_id`/`output_artifact_id` and dropping `model_sha256`. Since
the project has no deployed data (confirmed by the prior
`migrate-postgres-to-sqlite` change's same reasoning), each rebuild is a
plain `CREATE ... AS SELECT`-free `CREATE TABLE` + `DROP TABLE` pair with no
data migration step required.
- **Why**: Matches the existing migration-per-table convention and SQLite's
  actual `ALTER TABLE` capabilities; splitting into three keeps each
  migration's `up`/`down` pair reviewable against one table's change.
- **Alternative considered**: a single combined migration touching all three
  tables — fewer files, but harder to review/revert independently, and
  breaks the established one-migration-per-table pattern for no benefit
  here.

### HTTP layer: new `artifacts` and `models` route modules under the existing router
`http.rs`'s `router()` gains nested routers for `/api/v1/artifacts/*` and
`/api/v1/models/*`, backed by new `src/artifacts_api.rs` and
`src/models_api.rs` handler modules (kept separate from the `artifact_store`/
`model_registry` domain modules, matching the existing separation between
`http.rs` and `runs.rs`/`db.rs`). Content/download handlers use Axum's
streaming `Body::from_stream` over a `tokio::io::ReaderStream`, so a
multi-gigabyte log download never materializes fully server-side; the
download route sets `Content-Disposition: attachment` with a filename built
from `original_filename` (falling back to `<kind>-<sha256 prefix>` when
absent) and a rejects any path separator in that name before use.
- **Why**: Keeps the same module-per-concern shape the project already uses;
  streaming response bodies directly implements the proposal's "content
  responses should stream from disk" requirement.

### Maintenance/integrity report: synchronous scan behind a library function, exposed as a CLI-callable check (no new endpoint)
`integrity::check(pool, roots) -> IntegrityReport` walks `artifacts`/
`model_assets` rows against disk and walks the `artifacts/`/`models/`
directory trees against rows, returning the four categories from the spec.
It's invoked from a `#[tokio::main]`-callable path (e.g. `main.rs` behind an
`--integrity-check` arg, or a `#[test]`/example binary) rather than an HTTP
route, since the proposal frames it as "a maintenance operation," not part
of the public API surface the ingestion-service spec's endpoint list covers.
- **Why**: Keeps the report out of the authenticated-or-not HTTP surface
  question entirely (no auth system exists to protect a new route, and the
  proposal says not to design one), while still satisfying "provide a
  read-only command or report."
- **Alternative considered**: an HTTP `GET /api/v1/maintenance/integrity`
  route — more discoverable/scriptable over the network, but adds an
  unauthenticated introspection endpoint that can enumerate storage-layout
  details; deferred until the service has any authentication story.

## Risks / Trade-offs

- [Risk] Rebuilding `artifacts` and `runs` via drop-and-recreate migrations
  is only safe because no database has been deployed against the current
  schema yet (same precondition the prior SQLite migration change relied
  on). -> Mitigation: verified by re-checking `schema_metadata`/git history
  before writing the migrations; if that assumption is ever wrong for a
  running deployment, these migrations would need to become data-preserving
  rebuilds instead.
- [Risk] Tee-hashing before compression adds a second pass of borrowing over
  each chunk (hash, then compress) instead of hashing the already-compressed
  bytes in one pass. -> Mitigation: both are streaming, single-pass-over-the
  -stream operations (no re-reading), so the cost is one extra hashing
  operation's CPU time per chunk, not additional I/O or memory.
- [Risk] `model_asset_id NOT NULL` means any caller that previously recorded
  a run with only `model_sha256` (no prior registration step) can no longer
  insert a run without first registering a model asset. -> Mitigation: this
  is the proposal's explicit intent ("never copy a model for every run" via
  a real reference); registration is a one-time, idempotent (dedup-by-hash)
  call per unique model, not a per-run cost.
- [Risk] Deferring managed-mode's implementation means the `storage_mode =
  'managed'` path is schema-valid but has no code path to reach it yet.
  -> Mitigation: explicitly called out as deferred in this design and the
  proposal's Impact section rather than left implicit; the `CHECK` constraint
  still prevents an inconsistent managed row (missing `relative_path`) from
  ever being written by mistake.

## Open Questions

None — the scope, deferred managed-mode boundary, and schema shape above are
resolved decisions for this change, not open unknowns.
