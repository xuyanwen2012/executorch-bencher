# benchmark-schema Specification

## Purpose

Defines the authoritative relational data model for benchmark campaigns, runs,
and their results, so that benchmark data is reproducible, comparable across
runs, and safe from accidental overwrite or silent conflation of incompatible
conditions.

## Requirements

### Requirement: Device state is captured as an immutable snapshot
The system SHALL record the device state at the time of a run as fields on
that run's own immutable record, identified solely by device serial, rather
than referencing a mutable "current device" record, so that a device change
after a run does not change what that run is understood to have run on.

#### Scenario: Same device produces two different snapshots over time
- **WHEN** a device's BSP version is upgraded between two benchmark sessions
- **THEN** runs from the first session retain the old BSP version in their own
  row and runs from the second session retain the new BSP version in their
  own row, and neither row's captured device fields change after creation

#### Scenario: Snapshot captures reproducibility-relevant device fields
- **WHEN** a run is recorded
- **THEN** it includes, at minimum, the device serial, BSP version, SUMD
  driver version, device uptime in seconds, battery charging state,
  temperature at start, maximum temperature observed, and whether thermal
  throttling was detected

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

### Requirement: Each run attempt is an immutable, independently recorded row
The system SHALL record each execution attempt (including repeated
repetitions of the same command) as its own row that, once captured, is never
overwritten by a retry; a failed or crashed attempt SHALL remain in the
record as valid benchmark data.

#### Scenario: A crashed run is retried
- **WHEN** a run attempt crashes and the same command is executed again on
  the same device
- **THEN** the system records the retry as a new run row with its own
  repetition number, and the original crashed run row remains unchanged and
  queryable

#### Scenario: A run captures process-level outcome data
- **WHEN** a run finishes (successfully or not)
- **THEN** the system records its exit status (one of `succeeded`, `crashed`,
  `timed_out`, `cancelled`, or `infrastructure_error`), UTC start time, UTC
  finish time (nullable until the run completes), the exact command-line
  argument array executed, and references to captured stdout, stderr, and
  crash-log artifacts

#### Scenario: Duplicate repetitions are rejected
- **WHEN** a caller attempts to record two run rows sharing the same run ID
- **THEN** the system rejects the second write as a duplicate, since the run
  ID is the row's primary key; the MVP schema no longer ties uniqueness to a
  shared configuration/device grouping, so the repetition number itself is
  caller-assigned metadata rather than a database-enforced unique key

#### Scenario: A run records prefill and decode speed
- **WHEN** a run completes and reports throughput
- **THEN** the system records prefill speed in tokens per second as required
  and decode speed in tokens per second as nullable, so a run that has no
  decode phase can still be recorded

### Requirement: Correctness validation is tracked independently of process exit status
The system SHALL record the outcome of correctness validation for a run as a
field independent of that run's process-level exit status, since a process
can exit successfully while producing an incorrect result.

#### Scenario: A run exits cleanly but fails validation
- **WHEN** a run completes with exit status `succeeded`, and validation
  subsequently determines the output does not match the expected result
- **THEN** the system records the run's exit status as `succeeded` and its
  correctness result as `failed`, independently, without altering the exit
  status field

#### Scenario: Correctness values are constrained
- **WHEN** a correctness result is recorded for a run
- **THEN** the system only accepts one of `passed`, `failed`, `not_checked`,
  or `validator_error`, and rejects any other value

### Requirement: Captured run data is immutable; corrections are additive
The system SHALL treat a run's captured provenance and results as append-only
once written. A correction to previously captured data SHALL be recorded as a
new, distinct run row rather than by modifying a previously written run's
fields in place.

#### Scenario: A metric parser bug is fixed after data collection
- **WHEN** a bug in the benchmark runner's result-parsing logic is discovered
  after runs have already been recorded, and a corrected parser produces
  different prefill/decode speed values for the same command
- **THEN** the system preserves the originally recorded run row unchanged and
  represents the corrected values as a new run row rather than modifying the
  original record in place

### Requirement: Run metadata captures reproducibility inputs and provenance
The system SHALL record, for each run, a run ID, UTC start time, UTC finish
time (nullable until completed), a repetition number, the exact command-line
argument array, a human-readable command line, input parameters, an
environment-variable allowlist version, and the runner/collector version.

#### Scenario: Command arguments and parameters are stored as validated JSON
- **WHEN** a run is recorded with a command-line argument array and input
  parameters
- **THEN** the system stores each as canonical JSON text and rejects the
  write if either value is not valid JSON

#### Scenario: A run's finish time is absent until it completes
- **WHEN** a run is recorded while still in progress
- **THEN** the system stores a null finish time and populates it only when
  the run completes

### Requirement: Only an explicit environment-variable allowlist is captured
The system SHALL capture experiment-relevant environment variables only from
a small, explicit allowlist, and SHALL NOT capture the complete process
environment. Captured values SHALL be preserved exactly, distinguishing an
unset variable from an empty value where the runner protocol supports it.

#### Scenario: An unlisted environment variable is not captured
- **WHEN** a run executes with environment variables outside the configured
  allowlist
- **THEN** the system does not record those variables' names or values

#### Scenario: An unset allowlisted variable is distinguished from an empty one
- **WHEN** an allowlisted environment variable is unset in one run and set to
  an empty string in another run
- **THEN** the system records these two cases distinguishably rather than
  collapsing them to the same stored value

### Requirement: Performance configuration is captured per run
The system SHALL record, for each run, exactly three pinned clocks: GPU
clock, MIF (memory-interface) clock, and INT (interconnect) clock, each
expressed in MHz as an integer. No CPU-cluster, NPU, DSP, or general
memory-frequency fields are part of this requirement.

#### Scenario: Performance configuration defaults to documented values
- **WHEN** a run is recorded without an explicit GPU, MIF, or INT clock value
- **THEN** the system records the documented default for that clock (980 MHz
  for GPU, 5333 MHz for MIF, 934 MHz for INT) rather than leaving it unset

#### Scenario: Clock values must be positive
- **WHEN** a run is recorded with a zero or negative GPU, MIF, or INT clock
  value
- **THEN** the system rejects the write

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

### Requirement: Schema version is tracked in the database
The system SHALL record the currently applied schema version in a dedicated
`schema_metadata` record, distinct from the runs and artifacts tables.

#### Scenario: A fresh database records its schema version after migration
- **WHEN** migrations are applied to a database with no prior schema
- **THEN** the system records the resulting schema version in
  `schema_metadata`, queryable independently of any run or artifact data

### Requirement: Runs record optional git commit metadata
The system SHALL allow each run to record, in addition to its git commit
SHA and dirty flag, the branch name the run was made from, the commit's
author or committer timestamp as UTC RFC 3339 text, and the commit's
subject line. Each of these three values SHALL be nullable: a run recorded
without them SHALL be accepted, and runs recorded before this metadata
existed SHALL remain readable with null values.

#### Scenario: A run records its commit metadata
- **WHEN** a run is recorded with a branch name, commit timestamp, and
  commit subject
- **THEN** the system stores all three and returns them exactly when the
  run is read back

#### Scenario: A run without commit metadata is accepted
- **WHEN** a run is recorded with no branch, commit timestamp, or subject
- **THEN** the system accepts the write and reads the three values back as
  null

#### Scenario: Existing runs survive the migration
- **WHEN** the migration adding the metadata columns is applied to a
  database that already contains runs
- **THEN** every existing run remains readable, with null commit metadata
