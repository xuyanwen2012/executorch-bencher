## MODIFIED Requirements

### Requirement: Service starts with a database connection
The system SHALL, on startup, open a connection pool to the local SQLite
database file using externally supplied configuration, creating the file and
its containing directory if they do not already exist, and SHALL resolve and
create the configured artifact root, model root, temporary directory, and
trash directory. The service SHALL fail to start with a clear error,
identifying which configured root is unusable, if the database file or any
configured storage root cannot be opened, created, or written to — the
service SHALL NOT silently substitute a different location.

#### Scenario: Database is unreachable at startup
- **WHEN** the service is started and the configured SQLite database file
  path cannot be opened or created (for example, due to a permissions error
  or an invalid path)
- **THEN** the service exits with a non-zero status and an error message
  identifying the failure, rather than starting in a broken state

#### Scenario: Database is reachable at startup
- **WHEN** the service is started and the configured SQLite database file can
  be opened or created
- **THEN** the service establishes a connection pool and proceeds to accept
  HTTP requests

#### Scenario: A configured storage root is unusable
- **WHEN** the service is started and the configured artifact root, model
  root, temporary directory, or trash directory cannot be created or is not
  writable
- **THEN** the service exits with a non-zero status and an error message
  identifying which root failed, rather than falling back to a different
  directory or starting without it

## ADDED Requirements

### Requirement: Service exposes artifact upload, metadata, and content retrieval
The system SHALL expose HTTP operations to upload an artifact, fetch an
artifact's metadata, stream an artifact's content (decompressing when
necessary), and download an artifact with a safe filename. Content and
download responses SHALL stream from disk, set an appropriate content type,
and SHALL NOT expose the server's underlying filesystem path. When an
artifact's database record exists but its file is missing on disk, the
system SHALL respond with a clear, distinct error rather than a generic
failure or a silently empty body.

#### Scenario: Uploading an artifact returns its ID
- **WHEN** a client uploads artifact bytes with a valid kind
- **THEN** the system streams the content into storage and responds with the
  resulting artifact's ID

#### Scenario: An oversized upload is rejected
- **WHEN** a client uploads content exceeding the configured maximum artifact
  upload size
- **THEN** the system rejects the upload rather than streaming an unbounded
  amount of data to disk

#### Scenario: Content retrieval decompresses transparently
- **WHEN** a client requests the content of a Zstandard-compressed artifact
- **THEN** the system streams back the decompressed original bytes

#### Scenario: Retrieval of an artifact with a missing file is reported clearly
- **WHEN** a client requests the content or download of an artifact whose
  database record exists but whose file is absent from disk
- **THEN** the system responds with a distinct, clear error identifying the
  file as unavailable, rather than an unhandled failure

### Requirement: Service exposes model registration and verification
The system SHALL expose HTTP operations to register an external model asset,
list registered models, fetch a single model's metadata, and trigger explicit
full verification of a model's checksum.

#### Scenario: Registering a model returns its asset ID
- **WHEN** a client registers a valid external `.pte` file path
- **THEN** the system validates, hashes, and records the model, and responds
  with the resulting `model_assets` ID, reusing an existing record when the
  computed SHA-256 already exists

#### Scenario: Explicit verification is available on demand
- **WHEN** a client requests verification of a registered model
- **THEN** the system recalculates its SHA-256 from the current file content,
  updates its last-verified timestamp, and reports whether the checksum
  still matches the registered value

### Requirement: Run responses expose viewable artifact metadata
The system SHALL include, for each artifact referenced by a run, its
artifact ID, kind, original filename, size, media type, compression mode,
availability, and a route through which its content or download can be
retrieved, so a run's input, output, stdout, stderr, and crash information
can later be displayed or downloaded.

#### Scenario: A run's response lists its associated artifacts
- **WHEN** a client fetches a run that has stdout, output, and crash
  artifacts recorded
- **THEN** the response includes each artifact's kind, size, media type,
  compression mode, availability, and retrieval route, without requiring a
  separate lookup to discover that the artifact exists
