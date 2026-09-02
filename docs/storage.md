# Local storage design

The backend is entirely self-contained: SQLite for searchable metadata and
relationships, the local filesystem for large content. No Docker,
PostgreSQL, S3/MinIO, or other cloud/external service is used.

## Directory layout

Everything lives under a configurable data root (`DATA_ROOT` in `.env`,
`data/` by default). Each subpath can also be overridden independently
(`ARTIFACT_ROOT`, `MODEL_ROOT`, `TEMPORARY_DIR`, `TRASH_DIR`):

```text
data/
├── benchmarks.sqlite3           # SQLite database (DATABASE_URL)
├── artifacts/
│   └── sha256/
│       └── <first-two-hash-characters>/
│           └── <complete-sha256>     # content-addressed artifact blobs
├── models/
│   └── sha256/
│       └── <first-two-hash-characters>/
│           └── <complete-sha256>     # managed-mode model copies (future; unused today)
├── temporary/                        # in-flight uploads, cleaned by cleanup_abandoned_temp_files
└── trash/                            # reserved for a future move-before-delete GC step; unused today
```

All four directories are created automatically at startup
(`Config::prepare_storage_roots`). Startup fails with a clear error naming
the specific root if any of them can't be created or written to - it never
silently falls back to a different location.

SQLite is opened with `PRAGMA foreign_keys = ON`, `journal_mode = WAL`,
`busy_timeout = 5000`, and `synchronous = FULL` on every connection the pool
opens (`src/db.rs`).

## Artifact deduplication

Every backend-managed artifact (`prompt`, `stdout`, `stderr`, `output`,
`crash_log`, `logcat`, `correctness_report`) is content-addressed: its
storage path is `sha256/<prefix>/<full-hash>`, derived only from its own
SHA-256, never from any caller-supplied field (filename, kind, or anything
else in the request). Two artifacts with the same `(sha256, size_bytes)`
are the same row - `UNIQUE (sha256, size_bytes)` on `artifacts` enforces
this, and `store_artifact` reuses the existing row instead of inserting a
duplicate.

Ingestion streams the incoming bytes to a temporary file under
`temporary/`, hashing incrementally, then atomically renames it into its
final content-addressed path, and only *then* inserts (or reuses) the
database row - so a database row is never created for content that failed
to land at its expected path, and an interrupted upload leaves no row at
all. Concurrent uploads of identical content converge on one file and one
row; the loser of the rename/insert race reuses what the winner produced.

Text-oriented log kinds (`stdout`, `stderr`, `crash_log`, `logcat`,
`correctness_report`) are stored Zstandard-compressed. `prompt` and
`output` are stored exactly as given. In both cases, the recorded SHA-256
identifies the **original, uncompressed** content - compression is purely a
storage-layer detail; retrieval decompresses transparently and
incrementally, so a client never needs to know whether a given artifact
happens to be compressed on disk.

## Model registration

Large `.pte` model files are registered once in the `model_assets` table
and referenced by any number of runs via `runs.model_asset_id` - never
copied per run.

### External mode (default)

The default and only implemented mode today, matching the current ~14 GB
`.pte` models. Registration validates the path is a regular file, streams
it once to compute its SHA-256 and size, and records its path, size,
modification time, and checksum - the file itself is never copied.
Registering the same file (or a different path with identical content)
twice reuses the existing `model_assets` row via its unique SHA-256.

### Managed mode (schema-defined, not yet implemented)

`model_assets.storage_mode` also accepts `'managed'`, and the schema's
`CHECK` constraint already enforces the invariant a managed row would need
(a `relative_path` under `models/sha256/...`, no `external_path`). The
`ModelStorage` trait (`src/model_registry.rs`) is written so a
`ManagedModelStorage` - copying a model once into `models/sha256/<prefix>/
<sha256>`, deduplicated by checksum exactly like artifact storage - can be
added later without touching the schema or any existing caller. It is
**not implemented** in this change: there is no code path that creates a
managed-mode row today.

## Checksum caching and re-verification

Recomputing a SHA-256 over a 14 GB file on every run would be unacceptably
expensive. Instead:

- **Before a run** (`ModelStorage::verify`): the registry stats the file
  and compares its current `(size_bytes, mtime)` against the values cached
  at the last registration/verification. Unchanged → the cached SHA-256 is
  reused without reading the file. Either differs → the file is rehashed,
  and the row's `sha256`/`size_bytes`/`file_modified_at` are updated (or,
  if the new hash collides with a different already-registered model, this
  row is marked unavailable rather than silently reusing the new identity
  under the old row). Missing file → the row is marked unavailable, no
  rehash.
- **On demand** (`ModelStorage::verify_full`, exposed as
  `POST /api/v1/models/{id}/verify`): always rehashes from the current file
  content and updates `last_verified_at`, regardless of cached size/mtime.

## API surface

```text
POST /api/v1/artifacts                      stream-upload an artifact (?kind=..., ?original_name=...)
GET  /api/v1/artifacts/{id}/metadata         kind, size, media type, compression, availability, routes
GET  /api/v1/artifacts/{id}/content          streamed, decompressed content
GET  /api/v1/artifacts/{id}/download         same, with a safe Content-Disposition filename

POST /api/v1/models/register                {"path": "/abs/path/to/model.pte"} (beneath MODEL_REGISTER_ROOTS)
GET  /api/v1/models                          list all registered models
GET  /api/v1/models/{id}                     one model's metadata
POST /api/v1/models/{id}/verify              full re-verification (always rehashes)

GET  /api/v1/runs/{id}                       a run plus its attached artifacts' and model's metadata
```

None of these routes are authenticated - the service has no authentication
system and is meant for a trusted lab network. Because registration is
therefore open to anyone who can reach the port, it only accepts absolute
`.pte` paths beneath the configured `MODEL_REGISTER_ROOTS` (default: the
model root), resolved with symlinks followed, so it cannot be used to
discover or hash arbitrary server files. The importer and other Rust
callers of `ExternalModelStorage::register` are not confined.

## Missing-file behavior

A database row and its file can, in principle, fall out of sync (the row
insert and the file rename are not part of one atomic operation across
both SQLite and the filesystem). When that happens:

- `GET /api/v1/artifacts/{id}/content` and `/download` respond `410 Gone`
  with a clear message, distinguishing "file missing" from any other
  error - never a generic 500 or a silently empty body.
- The [integrity report](#retention-and-garbage-collection) surfaces both
  possible orphan states (row without file, file without row) without
  touching either.

## Safe restoration

A complete backup consists of:

```text
1. The SQLite database file (and its -wal/-shm files, if present)
2. The managed artifacts directory (artifacts/)
3. The managed models directory, if managed mode is ever used (models/)
```

Restoring all three together to the same relative layout (or pointing
`DATABASE_URL`/`ARTIFACT_ROOT`/`MODEL_ROOT` at wherever they were restored)
is sufficient to fully recover: artifact and model rows reference their
files by relative, portable paths, never absolute host-specific ones.

**External model files are not part of this backup.** They are registered
by reference (an `external_path` on the host), not copied, so restoring the
database alone does not restore them - they require their own,
company-approved backup policy for whatever filesystem or storage system
currently holds them. After restoring a database, run
`cargo run --example integrity_check` to see which external models (if any)
are currently unavailable at their recorded paths.

## Retention and garbage collection

This change does not implement any automatic, irreversible deletion of
artifact or model files. `temporary/` is the one exception: abandoned
temp-upload files (left behind by an interrupted upload) can be swept by
`artifact_store::cleanup_abandoned_temp_files`, which only removes files
older than the configured retention (`TEMP_FILE_RETENTION_SECONDS`),
never an actively-in-progress upload.

Beyond that, storage/database consistency is surfaced through a read-only
report (`cargo run --example integrity_check`, or `integrity::check`
directly) covering:

- Artifact rows no run currently references
- Artifact or model rows whose file is missing on disk
- Files on disk with no matching database row
- External models currently marked unavailable

If a future change adds actual garbage collection, the plan (per the
proposal) is: move unreferenced files into `trash/` first, with a grace
period before permanent deletion - never delete directly from `artifacts/`
or `models/` as a side effect of a normal API operation.
