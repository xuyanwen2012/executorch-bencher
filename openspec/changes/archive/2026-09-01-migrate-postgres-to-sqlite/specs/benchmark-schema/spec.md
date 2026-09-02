## MODIFIED Requirements

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

### Requirement: Large inputs and outputs are tracked as content-addressed artifacts
The system SHALL record large files (stdout captures, stderr captures, and
crash logs) as metadata pointing at content stored in a local,
content-addressed artifact directory, identified canonically by a SHA-256
content hash, rather than storing the file contents in the database or
referencing external object storage.

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

## ADDED Requirements

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
working tree was dirty, the executable's SHA-256 hash, the model `.pte`
file's SHA-256 hash, the prompt text file's SHA-256 hash, the input token
count, and the output token count.

#### Scenario: SHA-256 values are validated at the application boundary
- **WHEN** a run is recorded with an executable, model, or prompt hash
- **THEN** the system accepts the value only if it is exactly 64 lowercase
  hexadecimal characters, and rejects it otherwise

#### Scenario: Token counts must be nonnegative
- **WHEN** a run is recorded with a negative input or output token count
- **THEN** the system rejects the write

### Requirement: Schema version is tracked in the database
The system SHALL record the currently applied schema version in a dedicated
`schema_metadata` record, distinct from the runs and artifacts tables.

#### Scenario: A fresh database records its schema version after migration
- **WHEN** migrations are applied to a database with no prior schema
- **THEN** the system records the resulting schema version in
  `schema_metadata`, queryable independently of any run or artifact data

## REMOVED Requirements

### Requirement: Benchmark campaigns group comparable runs
**Reason**: The MVP schema drops campaigns as a grouping concept; grouping
and comparison of runs is deferred until a real requirement for it is
demonstrated against actual usage.
**Migration**: No data exists to migrate. Runs remain independently queryable
by their own fields (device serial, git commit, start time, etc.) without a
campaign grouping.

### Requirement: Source state is captured with commit as authoritative
**Reason**: Replaced by build/workload identity fields captured directly on
each run (see "Build and workload identity are captured per run"). The
separate snapshot entity and its reuse-by-matching-fields semantics are
dropped; the MVP schema does not track branch name or a build ID.
**Migration**: No data exists to migrate. Runs capture git commit SHA and
dirty state directly.

### Requirement: Run configurations capture intended, comparable test conditions
**Reason**: The MVP schema does not introduce a shared run-configuration
entity or a computed comparability fingerprint; each run stands on its own
captured fields. This is deferred until cross-run comparison is a proven,
concrete need.
**Migration**: No data exists to migrate.

### Requirement: Metrics are recorded through an extensible catalog, not fixed columns
**Reason**: The MVP only needs prefill and decode speed, which are captured
as fixed columns on `runs` (see "A run records prefill and decode speed").
An extensible metric catalog is deferred until more than these two
measurements is a proven need.
**Migration**: No data exists to migrate.

### Requirement: Telemetry is recorded as a time series per run
**Reason**: Out of scope for the MVP schema; only start temperature and
maximum temperature are captured as columns on `runs`. Sensor-tagged
time-series telemetry is deferred until justified by real usage.
**Migration**: No data exists to migrate.
