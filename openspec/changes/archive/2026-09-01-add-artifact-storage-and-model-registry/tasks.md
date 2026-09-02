## 1. Configuration and storage roots

- [x] 1.1 Extend `src/config.rs` with `data_root`, `artifact_root`, `model_root`, `temporary_dir`, `trash_dir`, `max_artifact_upload_bytes`, `output_preview_length`, and `temp_file_retention` fields, each independently overridable via its own env var with a default derived from `DATA_ROOT`, and verify with unit tests covering defaults, overrides, and missing/empty-value errors.
- [x] 1.2 On startup (`db::connect_and_migrate` caller in `main.rs`, plus a reusable `config::prepare_storage_roots` or similar), create all four storage directories and fail with a clear, root-identifying error if any is unusable; verify with a test that points one root at a blocked path (mirroring the existing `connect_and_migrate_fails_clearly_when_database_file_cannot_be_created` pattern) and asserts the error names that root.
- [x] 1.3 Update `.env` and `README.md` to document the new environment variables and the `benchmark-data/` layout.

## 2. Migrations

- [x] 2.1 Add a migration rebuilding `artifacts` with the widened `kind` CHECK (`prompt`, `stdout`, `stderr`, `output`, `crash_log`, `logcat`, `correctness_report`) and a new `compression TEXT NOT NULL DEFAULT 'none' CHECK (compression IN ('none','zstd'))` column, preserving existing column names/conventions (`storage_path`, `original_filename`); verify `cargo sqlx migrate run` against a fresh empty database succeeds and `PRAGMA table_info(artifacts)` shows the new column and constraint.
- [x] 2.2 Add a migration creating `model_assets` (id, sha256 UNIQUE, original_name, size_bytes, model_format DEFAULT 'pte', storage_mode, external_path, relative_path, file_modified_at, registered_at, last_verified_at, available) with the CHECK constraints described in design.md ("`runs.model_asset_id` is NOT NULL..." section) tying storage_mode to the correct path field; verify with a test inserting a valid external row, a valid managed row, and asserting an inconsistent row (e.g. `storage_mode='external'` with `relative_path` set) is rejected.
- [x] 2.3 Add a migration rebuilding `runs` to add `model_asset_id TEXT NOT NULL REFERENCES model_assets(id)`, `input_artifact_id TEXT REFERENCES artifacts(id)`, `output_artifact_id TEXT REFERENCES artifacts(id)`, and an `output_preview TEXT` column, dropping `model_sha256`; verify foreign-key rejection of a nonexistent `model_asset_id` and that `prompt_sha256` is retained unchanged.
- [x] 2.4 Write matching `.down.sql` files for each migration and verify `cargo sqlx migrate revert` (or the project's equivalent check) round-trips cleanly on a scratch database.
- [x] 2.5 Regenerate the `.sqlx` offline query cache per README instructions and commit it. (Nothing to regenerate: the codebase uses only runtime `query`/`query_as`/`query_scalar`, no `query!`/`query_as!` compile-time macros, so `.sqlx/` legitimately stays empty - confirmed via `grep -rn "query!\|query_as!\|query_scalar!" src/ tests/ examples/` returning no matches.)

## 3. Streaming artifact ingestion and compression

- [x] 3.1 Rewrite `src/artifact_store.rs`'s ingestion path to stream from an `AsyncRead` source into a temp file under the configured `temporary_dir`, computing SHA-256 and size incrementally without buffering the full content; verify with a test using a generated large (e.g. 200 MB sparse/streamed) source that memory-bounded streaming completes and the artifact is retrievable.
- [x] 3.2 Broaden `ArtifactKind` to the full seven-kind vocabulary and reject unknown kinds at the type/parsing boundary; verify with a unit test asserting an invalid kind string is rejected.
- [x] 3.3 Add Zstandard compression for `stdout`/`stderr`/`crash_log`/`logcat`/`correctness_report` kinds (hash-before-compress per design.md), leaving `prompt`/`output` uncompressed, and record `compression` on the artifact row; verify a round-trip test that stores a compressed artifact and asserts its recorded SHA-256 equals the hash of the original uncompressed bytes.
- [x] 3.4 Implement streaming, decompressing content retrieval (`get_artifact_content` returning an `AsyncRead`/stream) that never loads a full compressed or decompressed log into memory; verify with a test streaming a compressed artifact back out and comparing against the original bytes.
- [x] 3.5 Confirm/harden path resolution so no request-supplied field (original filename, kind) can influence the final storage path beyond display metadata; verify with a path-traversal test asserting a `../`-laden original filename does not escape the artifact root.
- [x] 3.6 Handle concurrent identical-content ingestion (parallel `store_artifact` calls with the same bytes) resulting in exactly one file and one artifact row; verify with a test spawning concurrent ingestions of identical content and asserting a single row/file.
- [x] 3.7 Implement recovery/cleanup for interrupted temp uploads (a temp file left behind with no corresponding artifact row) as a maintenance function that only removes temp files older than the configured retention, never an actively-written one; verify with a test seeding an old abandoned temp file and a fresh one, running cleanup, and asserting only the old one is removed.
- [x] 3.8 Update existing call sites (`runs_results.rs` test, any in-process callers) for the new streaming `store_artifact` signature.

## 4. Model registry

- [x] 4.1 Add `src/model_registry.rs` with a `ModelStorage` trait (`register`, `verify`, `verify_full`, `resolve_content_path`) and an `ExternalModelStorage` implementation performing the five registration steps (validate regular file, stream-hash once, store path/size/mtime/sha256, dedupe by sha256, never copy); verify with a test registering a generated large (sparse/streamed, not 14 GB) file and asserting no copy exists under the model root.
- [x] 4.2 Implement dedup-by-SHA-256 reuse on re-registration (same or different path); verify with a test registering the same file twice and asserting one `model_assets` row.
- [x] 4.3 Implement pre-run cached-checksum verification (size+mtime compare, reuse cached SHA-256 when unchanged, rehash when either differs, mark unavailable when the file is missing); verify with three tests: unchanged file skips rehash (e.g. via a call-count guard or timing/behavioral assertion), a changed file (touch mtime or resize) triggers rehash and updates the record, and a moved/deleted file is marked unavailable.
- [x] 4.4 Implement `verify_full`, always rehashing and updating `last_verified_at` regardless of cached size/mtime; verify with a test asserting `last_verified_at` advances and the checksum is recalculated even when size/mtime are unchanged.
- [x] 4.5 Define (without implementing) the `ManagedModelStorage` shape behind the same trait, documented in module-level comments as deferred per design.md, so the abstraction compiles and is ready for a future change.

## 5. Run relationships

- [x] 5.1 Update `NewRun`/`Run` in `src/runs.rs` to replace `model_sha256` with `model_asset_id: Uuid`, add `input_artifact_id: Option<Uuid>` and `output_artifact_id: Option<Uuid>`, and add `output_preview: Option<String>`, updating `insert_run`/`get_run`/`row_to_run` accordingly; verify existing `runs_lifecycle.rs`/`runs_results.rs` tests pass after updating their fixtures.
- [x] 5.2 Update `tests/common/mod.rs`'s `seed_new_run` to register a model asset and set `model_asset_id`, keeping other fixtures working; verify all existing test suites relying on `seed_new_run` still compile and pass.
- [x] 5.3 Add a test attaching input, output, stdout, stderr, and crash artifacts to a single run and reading them all back via `get_run`.
- [x] 5.4 Add a test asserting an artifact referenced by two different runs is preserved (present and readable) after one of those runs is removed (or, if run deletion isn't otherwise implemented, after the second run is inserted, asserting the first run's reference is unaffected).
- [x] 5.5 Add/keep a test asserting the default `gpu_clock_mhz`/`mif_clock_mhz`/`int_clock_mhz` values remain `980`/`5333`/`934`.
- [x] 5.6 Populate `output_preview` (truncated to `output_preview_length`) when an `output` artifact is attached, keeping the full content only in the artifact; verify with a test asserting the run row's preview is bounded while the artifact's full content is longer.

## 6. HTTP API

- [x] 6.1 Add `src/artifacts_api.rs` with handlers for `POST /api/v1/artifacts`, `GET /api/v1/artifacts/{id}/metadata`, `GET /api/v1/artifacts/{id}/content`, `GET /api/v1/artifacts/{id}/download`, wired into `http::router`; verify with an end-to-end test uploading bytes and retrieving them via content and download routes.
- [x] 6.2 Enforce `max_artifact_upload_bytes` on the upload route; verify with a test asserting an oversized body is rejected before being fully written to disk.
- [x] 6.3 Return a distinct, clear error (not a generic 500) when an artifact's database record exists but its file is missing; verify with a test that deletes an artifact's file out-of-band and asserts the content/download routes report it clearly.
- [x] 6.4 Add `src/models_api.rs` with handlers for `POST /api/v1/models/register`, `GET /api/v1/models`, `GET /api/v1/models/{id}`, `POST /api/v1/models/{id}/verify`, wired into `http::router`; verify with an end-to-end test registering a model and fetching it back.
- [x] 6.5 Include artifact metadata (ID, kind, original filename, size, media type, compression, availability, retrieval route) in run-fetching responses; verify with a test asserting a fetched run's JSON includes its attached artifacts' metadata.
- [x] 6.6 Ensure no handler exposes a raw server filesystem path in any response body; verify by grepping response-serialization code paths and confirming only IDs/relative-path-derived routes are exposed.

## 7. Integrity and maintenance

- [x] 7.1 Implement an `integrity` module/function reporting unreferenced artifact records, artifact/model rows with missing files, files on disk without matching rows, and unavailable external models, without modifying any data; verify with a test seeding each of the four conditions and asserting the report lists all of them.
- [x] 7.2 Wire the integrity check into a startup-safe or explicitly invoked maintenance path (e.g. a CLI flag or example binary), not a public HTTP route; verify by running it against a freshly migrated empty database and against a database with seeded orphan conditions.

## 8. Documentation

- [x] 8.1 Update `README.md` (or add `docs/storage.md`) covering: storage-directory layout, artifact deduplication, model registration, external vs. managed model storage, checksum caching/re-verification, backup requirements (SQLite database + managed artifacts directory + managed models directory if used; external model files need their own backup policy), missing-file behavior, safe restoration, and retention/GC behavior (read-only report only, no automatic deletion).

## 9. Verification

- [x] 9.1 Run `cargo fmt` and verify no diff remains.
- [x] 9.2 Run `cargo test` and verify the full suite passes, including all new tests enumerated in sections 2-7.
- [x] 9.3 Run `cargo clippy --all-targets -- -D warnings` and verify it exits clean.
- [x] 9.4 Apply migrations to a fresh empty SQLite database (`cargo sqlx migrate run` against a scratch `DATABASE_URL`) and verify success.
- [x] 9.5 Perform an end-to-end artifact upload and retrieval against a running instance (or an integration test exercising the same path) and verify byte-for-byte content match.
- [x] 9.6 Register a representative external `.pte`-shaped file (generated/sparse, not a real 14 GB model) and verify via filesystem inspection that no copy was created under the model root.
- [x] 9.7 Restart the backend (or re-open the pool in a test against the same on-disk database/artifact root) and verify previously stored artifacts and model registrations remain readable.
- [x] 9.8 Run the integrity checker against the post-restart database and verify it reports no unexpected orphans.
- [x] 9.9 Report modified files, migration details, tests run, deferred functionality (managed model storage), and remaining risks, per design.md's Risks / Trade-offs.
