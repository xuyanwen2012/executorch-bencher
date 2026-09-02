# artifact-storage Specification

## Purpose

Defines the behavioral contract of the local content-addressed storage
engine: how artifact bytes and large `.pte` model files are ingested,
deduplicated, compressed, verified, and safely reconciled on disk,
independent of the relational schema that references them or the HTTP
surface that exposes them.

## Requirements

### Requirement: Artifact ingestion is streamed and content-addressed
The system SHALL ingest artifact bytes by writing them to a temporary file
under a dedicated temporary directory while computing their SHA-256 hash and
byte size incrementally, without buffering the complete content in memory,
then atomically moving the temporary file into its final content-addressed
location before any database record is created for it.

#### Scenario: A large artifact is ingested without full buffering
- **WHEN** an artifact is uploaded whose size exceeds available process
  memory for a single in-memory buffer
- **THEN** the system completes ingestion by streaming the content to disk
  and computing its hash incrementally, rather than allocating a buffer
  sized to the complete content

#### Scenario: No database row is created for an incomplete write
- **WHEN** ingestion is interrupted before the temporary file is fully
  written and moved into its final location
- **THEN** the system does not create or leave behind an `artifacts` row
  referencing that incomplete content, and the interrupted temporary file
  can be safely discarded

#### Scenario: Concurrent uploads of identical content converge on one artifact
- **WHEN** two ingestion requests stream the same content at the same time
- **THEN** exactly one file exists at the resulting content-addressed path
  and exactly one `artifacts` row identifies that content, with both
  requests returning the same artifact identity

#### Scenario: Abandoned temporary files are cleaned without touching active uploads
- **WHEN** a maintenance operation or startup cleanup runs while one upload
  is actively streaming into its temporary file and another temporary file
  was abandoned by a prior interrupted run
- **THEN** the system removes only the abandoned temporary file and leaves
  the actively streaming temporary file untouched

### Requirement: Backend-managed storage paths cannot be chosen by request data
The system SHALL derive every backend-managed artifact and model storage path
from the content's own SHA-256 hash and its configured root, and SHALL
resolve every such path to confirm it stays within that configured root
before any filesystem operation. No field supplied by a request (filename,
kind, or any other caller-provided value) SHALL be used, directly or
indirectly, to choose the final storage location.

#### Scenario: A crafted filename cannot escape the artifact root
- **WHEN** an ingestion request supplies an original filename or other field
  containing path-traversal sequences (e.g. `../../etc/passwd`)
- **THEN** the system stores the content only at its computed
  content-addressed path beneath the configured artifact root, and the
  supplied filename is retained solely as display metadata

### Requirement: Artifact kind determines whether content is compressed
The system SHALL compress artifact content with Zstandard when its kind is
one of `stdout`, `stderr`, `crash_log`, `logcat`, or `correctness_report`,
and SHALL store `prompt` and `output` artifacts uncompressed. The recorded
SHA-256 identity SHALL always be computed over the original, uncompressed
bytes, regardless of whether the stored file is compressed.

#### Scenario: A compressed log's identity matches its uncompressed content
- **WHEN** a `stdout` artifact is ingested and stored Zstandard-compressed
- **THEN** its recorded SHA-256 equals the hash of the original uncompressed
  bytes, not the hash of the compressed file on disk

#### Scenario: Compressed content is decompressed as a stream on retrieval
- **WHEN** a compressed artifact's content is requested
- **THEN** the system decompresses it incrementally while streaming the
  response, without loading the complete decompressed content into memory
  first

### Requirement: External model assets are registered once without copying
The system SHALL register a `.pte` model file in external mode by validating
that the path exists and is a regular file, streaming it once to compute its
SHA-256 and byte size, and recording its path, size, modification time, and
checksum — without copying the file into the managed storage roots. A model
already registered under the same SHA-256 SHALL be reused rather than
duplicated, and any number of runs SHALL be able to reference the same model
asset record.

#### Scenario: Registering the same model twice reuses one record
- **WHEN** the same `.pte` file is registered twice, whether at the same path
  or a different path
- **THEN** the system reuses the existing `model_assets` row identified by
  its SHA-256 rather than creating a second record

#### Scenario: Registration never copies the model file
- **WHEN** a large external model is registered
- **THEN** no copy of its bytes is written under the configured model root,
  and the registered record stores only its external path

### Requirement: External model verification avoids unnecessary rehashing
Before a run uses a registered external model, the system SHALL confirm the
path still exists, compare its current size and modification time against
the values stored at registration or last verification, and reuse the cached
SHA-256 when both are unchanged. When either has changed, the system SHALL
recalculate the SHA-256 rather than silently continuing to associate the run
with the previously cached identity, and SHALL mark the asset's availability
accordingly. The system SHALL also expose an explicit full-verification
operation that recalculates the SHA-256 on demand and updates the asset's
last-verified timestamp, independent of whether size or modification time
changed.

#### Scenario: An unchanged model is not rehashed before a run
- **WHEN** a run is about to use a registered external model whose recorded
  size and modification time both still match the file on disk
- **THEN** the system reuses the cached SHA-256 without reading the file's
  content

#### Scenario: A changed model is detected and rehashed
- **WHEN** a registered external model's file size or modification time no
  longer matches the recorded values
- **THEN** the system recalculates the SHA-256 before the run proceeds and
  does not associate the run with the stale cached identity

#### Scenario: A moved or deleted external model is reported, not silently accepted
- **WHEN** a registered external model's path no longer resolves to an
  existing regular file
- **THEN** the system marks the asset unavailable rather than treating the
  run as having used the previously registered identity

#### Scenario: Explicit full verification updates the last-verified timestamp
- **WHEN** an operator triggers full verification of a registered model
- **THEN** the system recalculates its SHA-256 from the current file content
  and records the verification's completion time, regardless of whether size
  or modification time had changed

### Requirement: Storage integrity is reconciled through a read-only report, not automatic deletion
The system SHALL provide a maintenance operation that reports, without
modifying any data: artifact records with no file at their expected
location, files present on disk with no matching database record,
unreferenced artifact records, and external model assets that are currently
unavailable. The system SHALL NOT perform automatic, irreversible deletion of
artifact or model files as part of normal operation.

#### Scenario: A missing file is reported without deleting its database row
- **WHEN** the integrity report runs and finds an `artifacts` row whose
  content-addressed file is absent from disk
- **THEN** the report lists that row as having a missing file, and the row
  itself is left unchanged

#### Scenario: An orphaned file is reported without being deleted
- **WHEN** the integrity report runs and finds a file under the artifact or
  model storage root with no corresponding database record
- **THEN** the report lists that file as orphaned, and the file is left in
  place

### Requirement: Shared artifacts and model assets outlive any single referencing run
The system SHALL NOT delete an artifact's or model asset's stored file or
database record as a side effect of deleting, modifying, or superseding any
one run that references it. A physical deletion of an artifact or model file
SHALL occur only through an operation that first confirms no run references
it.

#### Scenario: Removing one run's reference does not delete a shared artifact
- **WHEN** an artifact is referenced by more than one run and one of those
  runs is deleted
- **THEN** the artifact's file and database record remain intact and
  retrievable through the runs that still reference it
