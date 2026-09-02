## Why

Runs currently reference only three artifact kinds (`stdout`, `stderr`,
`crash_log`), artifacts are stored in memory and written non-atomically under
`tmp/` inside the artifact root, and there is no way to attach a run's prompt
or generated output as durable, retrievable content — `model_sha256` and
`prompt_sha256` are bare hashes with no backing bytes. Every run also has no
way to reference the `.pte` model it exercised except by hash; nothing
prevents a future design from copying a 14 GB model file per run, and nothing
lets a run's stdout/stderr/crash-log/prompt/output later be displayed or
downloaded through the service. This change closes that gap entirely inside
the existing local SQLite + filesystem architecture: no Docker, Postgres, or
cloud storage is introduced.

## What Changes

- Expand the artifact-store implementation to stream ingestion (temp file →
  hash while streaming → atomic rename → dedup-or-insert) instead of
  buffering complete files in memory, using a dedicated `temporary/`
  directory (not a subdirectory of `artifacts/`) and a `trash/` directory for
  future safe deletion.
- Broaden the `artifacts` table's `kind` vocabulary from
  `stdout`/`stderr`/`crash_log` to also include `prompt`, `output`, `logcat`,
  and `correctness_report`, and add a `compression` column (`none`/`zstd`)
  so text-oriented logs (`stdout`, `stderr`, `crash_log`, `logcat`,
  `correctness_report`) can be stored Zstandard-compressed while `prompt` and
  `output` stay uncompressed; content identity (the stored SHA-256) is always
  computed over the original uncompressed bytes.
- Add a `model_assets` table and model-registry module supporting **external**
  mode (default; validates, hashes once, and records a `.pte` file's path/
  size/mtime/SHA-256 without copying it, with change detection via cached
  size+mtime and an explicit re-verify operation) and a storage abstraction
  for a future **managed** mode (content-addressed copy under
  `models/sha256/...`), fully specified but not required to ship a managed
  code path if it would add substantial complexity beyond external mode.
- Add `model_asset_id`, `input_artifact_id`, and `output_artifact_id`
  foreign-key columns to `runs` (joining the existing
  `stdout_artifact_id`/`stderr_artifact_id`/`crash_artifact_id` columns),
  replacing the bare `model_sha256` hash with a reference to a registered
  model asset. **BREAKING**: `runs.model_sha256` is removed in favor of
  `model_asset_id`; `prompt_sha256` is retained as a denormalized checksum
  but `input_artifact_id` becomes the source of truth for the exact prompt
  text.
- Add HTTP endpoints for artifact upload/metadata/content/download and model
  register/list/get/verify, all streaming from disk (decompressing logs on
  the fly) and never exposing server filesystem paths.
- Add explicit configuration for the database path, artifact root, model
  root, temporary directory, trash directory, max upload size, output-preview
  length, and temporary-file retention, with startup validation that fails
  loudly (rather than silently falling back) if any configured root is
  unusable.
- Add a read-only integrity/reconciliation report (orphaned temp files,
  artifact rows with missing files, files with no row, unavailable external
  models) as a maintenance operation; no automatic destructive cleanup is
  introduced in this change.

## Capabilities

### New Capabilities
- `artifact-storage`: Content-addressed artifact ingestion (streaming,
  dedup, compression), the external/managed model registry, temp-file and
  trash lifecycle, and integrity reporting — the storage engine's behavioral
  rules, independent of the HTTP surface or the relational schema that
  references it.

### Modified Capabilities
- `benchmark-schema`: `artifacts.kind` vocabulary and `compression` column;
  new `model_assets` table; `runs` gains `model_asset_id`,
  `input_artifact_id`, `output_artifact_id` and loses `model_sha256`.
- `ingestion-service`: new artifact and model-registry HTTP endpoints;
  startup configuration/validation for the additional storage roots.

## Impact

- **Code**: `src/artifact_store.rs` (rewritten for streaming + compression),
  new `src/model_registry.rs`, `src/http.rs` (new routes), `src/runs.rs`
  (updated `NewRun`/`Run`), `src/config.rs` (new roots + limits), `src/db.rs`
  (unchanged pragmas, new migrations applied through it).
- **Migrations**: new migration(s) altering `artifacts`, creating
  `model_assets`, and altering `runs`.
- **Dependencies**: adds a Zstandard crate (e.g. `zstd`) and streaming
  file-hashing support; no new external services.
- **Tests/examples**: `tests/*.rs` and `examples/*.rs` gain coverage for
  streaming ingestion, dedup, model registration/verification, and FK
  enforcement across the new columns; `.env`/README updated for new
  configuration.
