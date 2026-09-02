## Purpose

Defines the authoritative relational data model for benchmark campaigns, runs,
and their results, so that benchmark data is reproducible, comparable across
runs, and safe from accidental overwrite or silent conflation of incompatible
conditions.

## ADDED Requirements

### Requirement: Benchmark campaigns group comparable runs
The system SHALL let a user define a benchmark campaign identifying a named
comparison exercise, including an optional baseline commit reference, a
description, and the identity of the user who created it.

#### Scenario: Campaign is created with a baseline
- **WHEN** a user creates a campaign named "Q3 device sweep" with a baseline
  commit reference
- **THEN** the system persists the campaign with a unique identifier, the
  supplied name, baseline commit, and creator, and records a creation
  timestamp

### Requirement: Device state is captured as an immutable snapshot
The system SHALL record the full hardware/software state of a device at the
time of a run as its own immutable record, rather than referencing a mutable
"current device" record, so that a device upgrade after a run does not change
what that run is understood to have run on.

#### Scenario: Same device produces two different snapshots over time
- **WHEN** a device's BSP version is upgraded between two benchmark sessions
- **THEN** runs from the first session reference a snapshot recorded with the
  old BSP version and runs from the second session reference a distinct
  snapshot recorded with the new BSP version, and neither snapshot's captured
  fields change after creation

#### Scenario: Snapshot captures reproducibility-relevant device fields
- **WHEN** a device snapshot is captured
- **THEN** it includes, at minimum, a device identifier, board model, BSP
  version, kernel version, and driver/firmware version information, along
  with the timestamp at which the state was captured

### Requirement: Source state is captured with commit as authoritative
The system SHALL record the source code state used for a run as a snapshot
that treats the commit hash as authoritative, while also recording the branch
name, working-tree dirty state, and build identifiers/options, since branch
names can move but commits do not.

#### Scenario: Dirty working tree is recorded, not rejected
- **WHEN** a benchmark is run against an uncommitted, modified working tree
- **THEN** the system records the snapshot as dirty and retains a reference to
  the diff that was in effect, rather than refusing to record the run

#### Scenario: Identical source state is not duplicated
- **WHEN** two runs are recorded against the same repository, commit, dirty
  state, and build identifier
- **THEN** the system reuses the same source snapshot record for both rather
  than creating a duplicate

### Requirement: Large inputs and outputs are tracked as content-addressed artifacts
The system SHALL record large files (inputs, models, logs, outputs, crash
dumps) as metadata pointing at externally stored content, identified
canonically by a cryptographic content hash, rather than storing the file
contents in the relational store.

#### Scenario: Artifact metadata is queryable without fetching the file
- **WHEN** an artifact is registered with its storage location, size, and
  content hash
- **THEN** the system can answer queries about the artifact (its kind, size,
  and hash) without retrieving the underlying file from object storage

#### Scenario: Identical file contents are recognized as the same artifact
- **WHEN** two uploads produce files with the same content hash and size
- **THEN** the system treats them as the same artifact record rather than
  registering a duplicate

### Requirement: Run configurations capture intended, comparable test conditions
The system SHALL let a user define a run configuration that ties together a
campaign, a source snapshot, an optional input artifact, a model
identification, a command template, and arbitrary parameters/environment
settings, and SHALL compute a comparability fingerprint over every setting
that affects results.

#### Scenario: Two configurations with different comparability-relevant settings are distinguishable
- **WHEN** two run configurations differ in any field that affects results
  (for example, a parameter value or environment setting)
- **THEN** their comparability fingerprints differ

#### Scenario: Two configurations with identical result-affecting settings match
- **WHEN** two run configurations are created with the exact same
  result-affecting settings
- **THEN** their comparability fingerprints are equal, signaling that their
  results are directly comparable

### Requirement: Each run attempt is an immutable, independently recorded row
The system SHALL record each execution attempt (including repeated
repetitions of the same configuration) as its own row that, once captured, is
never overwritten by a retry; a failed or crashed attempt SHALL remain in the
record as valid benchmark data.

#### Scenario: A crashed run is retried
- **WHEN** a run attempt crashes and the same configuration is executed again
  on the same device
- **THEN** the system records the retry as a new run row with an incremented
  repetition number, and the original crashed run row remains unchanged and
  queryable

#### Scenario: A run captures process-level outcome data
- **WHEN** a run finishes (successfully or not)
- **THEN** the system records its status (one of a fixed set of outcomes
  including success, crash, timeout, cancellation, invalid output, or
  infrastructure error), start/finish times, the exact command executed,
  exit code or termination signal, and references to captured stdout,
  stderr, and output artifacts

#### Scenario: Duplicate repetitions are rejected
- **WHEN** a caller attempts to record two runs with the same configuration,
  device snapshot, and repetition number
- **THEN** the system rejects the second write as a duplicate

### Requirement: Metrics are recorded through an extensible catalog, not fixed columns
The system SHALL maintain a catalog of named metrics (with unit and
directionality metadata) and SHALL record each observed value against a run
as a reference to a catalog entry plus a value, rather than requiring a fixed,
ever-growing set of columns for every possible metric.

#### Scenario: A new metric type is introduced without a schema change
- **WHEN** a new kind of measurement (for example, "peak memory") is
  introduced
- **THEN** it can be recorded by adding a new catalog entry and associated
  observations, without altering the structure used to store existing metrics

#### Scenario: A run can report the same metric for multiple phases
- **WHEN** a run reports "latency" for both a "prefill" phase and a "decode"
  phase
- **THEN** the system stores both observations distinctly, keyed by run,
  metric, and phase

### Requirement: Telemetry is recorded as a time series per run
The system SHALL record telemetry samples (for example, temperature, clock
frequency, utilization) as timestamped, sensor-tagged observations tied to a
specific run, distinct from summary metrics.

#### Scenario: Multiple sensors report during one run
- **WHEN** a run produces temperature readings from one sensor and clock
  frequency readings from another sensor during its execution
- **THEN** the system stores each as a separate telemetry sample identified
  by run, timestamp, and sensor

### Requirement: Correctness validation is tracked independently of process exit status
The system SHALL record the outcome of correctness/output validation for a
run as a separate record from the run's process-level status, since a
process can exit successfully while producing an incorrect result.

#### Scenario: A run exits cleanly but fails validation
- **WHEN** a run completes with a successful process exit code, and a
  validator subsequently determines the output does not match the expected
  result
- **THEN** the system records the run's process status as successful and
  independently records a failing validation result for that run, without
  altering the run's own status field

### Requirement: Captured run data is immutable; corrections are additive
The system SHALL treat a run's captured provenance and raw results as
append-only once written. Corrections to a previous interpretation (for
example, a fixed output parser producing a different derived value) SHALL be
recorded as new, distinctly attributed data rather than overwriting the
original captured values.

#### Scenario: A metric parser bug is fixed after data collection
- **WHEN** a bug in a metrics parser is discovered after runs have already
  been recorded, and a corrected parser produces different values
- **THEN** the system preserves the original recorded values and their
  origin, and represents the corrected values as newly attributed data
  rather than modifying the original records in place
