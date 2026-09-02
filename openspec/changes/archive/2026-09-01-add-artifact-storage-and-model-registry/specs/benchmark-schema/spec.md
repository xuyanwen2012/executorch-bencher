## MODIFIED Requirements

### Requirement: Large inputs and outputs are tracked as content-addressed artifacts
The system SHALL record large files (prompt text, stdout captures, stderr
captures, generated output, crash logs, device logcat captures, and
correctness reports) as metadata pointing at content stored in a local,
content-addressed artifact directory, identified canonically by a SHA-256
content hash, rather than storing the file contents in the database or
referencing external object storage. Each artifact record SHALL also carry
its compression mode, since text-oriented log kinds are stored
Zstandard-compressed while prompt and output content are stored
uncompressed.

#### Scenario: Artifact metadata is queryable without fetching the file
- **WHEN** an artifact is registered with its storage path, size, kind, and
  SHA-256 hash
- **THEN** the system can answer queries about the artifact (its kind, size,
  and hash) without reading the underlying file

#### Scenario: Identical file contents are recognized as the same artifact
- **WHEN** two writes produce files with the same SHA-256 hash and size
- **THEN** the system reuses the same artifact record rather than registering
  a duplicate, enforced by a uniqueness constraint on (SHA-256, size)

#### Scenario: Artifact writes never leave a dangling database reference
- **WHEN** an artifact is being stored
- **THEN** the system writes the content to a temporary location, verifies
  its SHA-256 hash, atomically moves it into its final content-addressed
  location, and only then inserts or reuses the database record - so no
  artifact row is ever created for a file that failed to land at its
  expected path

#### Scenario: Stored artifact paths are portable
- **WHEN** an artifact's storage path is recorded
- **THEN** it is stored relative to the configured artifact root, never as an
  absolute, machine-specific path

#### Scenario: Only the documented artifact kinds are accepted
- **WHEN** an artifact is registered with a `kind` value
- **THEN** the system accepts only `prompt`, `stdout`, `stderr`, `output`,
  `crash_log`, `logcat`, or `correctness_report`, and rejects any other value

#### Scenario: An artifact's compression mode is recorded and constrained
- **WHEN** an artifact record is created
- **THEN** the system records its compression mode as either `none` or
  `zstd`, and rejects any other value

### Requirement: Build and workload identity are captured per run
The system SHALL record, for each run, the git commit SHA, whether the
working tree was dirty, the executable's SHA-256 hash, a reference to the
registered model asset exercised by the run, the prompt text file's SHA-256
hash, the input token count, and the output token count.

#### Scenario: SHA-256 values are validated at the application boundary
- **WHEN** a run is recorded with an executable or prompt hash
- **THEN** the system accepts the value only if it is exactly 64 lowercase
  hexadecimal characters, and rejects it otherwise

#### Scenario: Token counts must be nonnegative
- **WHEN** a run is recorded with a negative input or output token count
- **THEN** the system rejects the write

#### Scenario: A run's model reference must resolve to a registered asset
- **WHEN** a run is recorded referencing a model asset ID that does not
  exist in `model_assets`
- **THEN** the system rejects the write via foreign-key enforcement

#### Scenario: Many runs share one model asset reference
- **WHEN** multiple runs exercise the same registered model
- **THEN** each run stores a reference to the same `model_assets` row rather
  than its own copy of the model's identity or content

## ADDED Requirements

### Requirement: Model assets are recorded in a dedicated registry
The system SHALL record registered `.pte` model files in a `model_assets`
table distinct from `runs` and `artifacts`, capturing the model's SHA-256
(unique), original filename, size, format, storage mode (`external` or
`managed`), registration time, last-verification time, and availability.
Exactly one of an external path or a managed relative path SHALL be present,
matching the record's storage mode: an `external` record SHALL carry an
external path and no relative path, and a `managed` record SHALL carry a
relative path and no external path.

#### Scenario: A model record's path fields match its storage mode
- **WHEN** a `model_assets` row is inserted with `storage_mode = 'external'`
  and no `external_path`, or with `storage_mode = 'managed'` and no
  `relative_path`
- **THEN** the system rejects the write

#### Scenario: A model's SHA-256 is unique across the registry
- **WHEN** a second `model_assets` row is inserted with a SHA-256 that
  already exists in the table
- **THEN** the system rejects the write, so registration logic must reuse the
  existing row instead of inserting a duplicate

### Requirement: A run references its model asset and input/output artifacts by foreign key
The system SHALL extend `runs` with a `model_asset_id` foreign key to
`model_assets`, and `input_artifact_id` and `output_artifact_id` foreign keys
to `artifacts`, alongside the existing `stdout_artifact_id`,
`stderr_artifact_id`, and `crash_artifact_id` columns. `input_artifact_id`
and `output_artifact_id` MAY be null when the exact prompt text or generated
output has not been captured as an artifact for that run.

#### Scenario: A run's prompt artifact preserves exact historical text
- **WHEN** a run records an `input_artifact_id` pointing at a `prompt`
  artifact, and the original prompt file on disk is later changed
- **THEN** the run's recorded prompt content remains exactly what was
  captured at run time, unaffected by the later change to the source file

#### Scenario: A run without a captured output leaves its output reference null
- **WHEN** a run is recorded before its generated output has been ingested as
  an artifact
- **THEN** the system stores a null `output_artifact_id` rather than
  rejecting the write

### Requirement: A run retains a short output preview alongside the complete artifact
The system SHALL store a short preview of a run's generated output as a text
field on the run row, bounded to a configured maximum length, while treating
the corresponding `output` artifact as the complete, authoritative record of
the generated content.

#### Scenario: The output preview is bounded independent of output size
- **WHEN** a run's generated output exceeds the configured preview length
- **THEN** the run row stores only the truncated preview, while the complete
  output remains fully readable through its `output` artifact
