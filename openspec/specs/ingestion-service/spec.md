# ingestion-service Specification

## Purpose

Provides the running HTTP service that will host the benchmark ingestion and
dashboard APIs, established here as a minimal, operable skeleton that later
changes extend with actual endpoints.

## Requirements

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

### Requirement: Every database connection enforces integrity and durability settings
The system SHALL apply foreign-key enforcement, write-ahead-log journaling,
a busy timeout, and full synchronous durability to every SQLite connection
it opens, since some of these settings are per-connection rather than
persistent in the database file.

#### Scenario: A foreign-key-violating write is rejected
- **WHEN** the service attempts to insert a row that references a
  nonexistent foreign key
- **THEN** the write is rejected rather than silently accepted

#### Scenario: A newly opened pooled connection enforces the same settings
- **WHEN** the connection pool opens an additional connection after startup
  (for example, to serve concurrent requests)
- **THEN** that connection also has foreign-key enforcement and the
  configured busy timeout applied before serving queries

### Requirement: Service applies pending schema migrations before serving traffic
The system SHALL ensure the database schema is up to date with the service's
expected schema version before accepting requests that depend on it.

#### Scenario: Fresh database has no schema yet
- **WHEN** the service starts against a database with no benchmark schema
  present
- **THEN** the service applies the schema definitions needed to reach the
  current expected version before accepting traffic

### Requirement: Service exposes a health check endpoint
The system SHALL expose an HTTP endpoint that reports whether the service is
running and able to reach its database.

#### Scenario: Health check succeeds when dependencies are healthy
- **WHEN** a client requests the health check endpoint while the service's
  database connection is healthy
- **THEN** the service responds with a success status

#### Scenario: Health check reflects a lost database connection
- **WHEN** a client requests the health check endpoint while the service
  cannot reach its database
- **THEN** the service responds with a failure status rather than reporting
  healthy

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

### Requirement: HTTP error responses use a consistent JSON envelope
Every failed or rejected HTTP request SHALL respond with a JSON body of
the form `{ "error": { "code": <stable string>, "message": <human text>,
"details": <optional object>, "request_id": <optional string> } }`, with
`code` drawn from a documented set that includes at least
`invalid_request`, `not_found`, `artifact_file_missing`,
`payload_too_large`, `conflict`, `not_implemented`, and `internal_error`,
and an HTTP status consistent with the code (`400`, `404`, `404`, `413`,
`409`, `501`, `500` respectively). Clients SHALL match on `code`, never on
`message`. The system SHALL NOT include SQLite error text, SQL
statements, absolute filesystem paths, secrets, or stack traces in this
response.

#### Scenario: An oversized artifact upload returns the envelope
- **WHEN** an upload exceeds the configured maximum size
- **THEN** the response is `413` with code `payload_too_large`

#### Scenario: A request for an unknown resource returns the envelope
- **WHEN** a client requests a run, artifact, or model that does not exist
- **THEN** the response is `404` with code `not_found`

#### Scenario: An artifact whose file is missing returns the envelope
- **WHEN** a client requests content for an artifact whose file is gone
- **THEN** the response is `404` with code `artifact_file_missing`

#### Scenario: An invalid artifact kind returns the envelope
- **WHEN** an upload names an unrecognized kind
- **THEN** the response is `400` with code `invalid_request`

#### Scenario: A duplicate run ID returns the envelope
- **WHEN** a run is posted with an ID that already exists
- **THEN** the response is `409` with code `conflict`

### Requirement: Service exposes grouped benchmark results
The system SHALL expose an HTTP operation that groups runs into benchmark
configurations and returns one result row per configuration. The
configuration key SHALL be the tuple of device serial, model asset, git
commit SHA, git dirty flag, SUMD driver version, BSP version, GPU clock,
MIF clock, INT clock, and prompt SHA-256. Statistics SHALL be computed only
over runs whose exit status is `succeeded`: for prefill throughput and,
separately, for decode throughput (over runs that recorded one), the
median, minimum, maximum, and count. Each row SHALL also carry the total
run count, the count of runs that did not succeed, the count of succeeded
runs whose correctness result is `failed`, the count of runs that reported
thermal throttling, the earliest and latest run start times, the input
token count, the model's ID and original name, and the commit's branch,
timestamp, and subject when recorded. Rows SHALL be ordered by commit
timestamp descending, falling back to the configuration's earliest run
start time when no commit timestamp is recorded, then by model name and
device serial. The operation SHALL accept exact-match filters on device
serial, model asset ID, git commit SHA, git branch, git dirty flag, SUMD
driver version, BSP version, and prompt SHA-256, combined conjunctively,
SHALL return at most 500 rows with a flag indicating truncation, and SHALL
return `facets`: the distinct device serials, models (ID and name), git
branches, SUMD driver versions, and BSP versions present across all runs
regardless of the active filters.

#### Scenario: Repetitions collapse into one row with a median
- **WHEN** five succeeded runs share one configuration key with prefill
  throughputs 100, 110, 120, 130, and 900
- **THEN** the results contain one row for that key with prefill median
  120, minimum 100, maximum 900, and count 5

#### Scenario: Only succeeded runs contribute to statistics
- **WHEN** a configuration has three succeeded runs and two crashed runs
- **THEN** its row reports statistics over the three succeeded runs, a
  total of 5, and a not-succeeded count of 2

#### Scenario: A configuration with no succeeded runs
- **WHEN** every run for a configuration key failed
- **THEN** its row is present with null throughput statistics, a count of
  0, and the failure count

#### Scenario: Dirty and clean runs of one commit are separate rows
- **WHEN** runs exist for the same commit SHA with and without a dirty
  working tree
- **THEN** the results contain two rows differing only in the dirty flag

#### Scenario: Rows are ordered by commit history when available
- **WHEN** runs exist for two commits with recorded commit timestamps and a
  third commit without one
- **THEN** the two timestamped commits appear newest first, and the third
  is positioned by its earliest run start time

#### Scenario: Facets ignore active filters
- **WHEN** a client requests results filtered to one device serial
- **THEN** `facets` still lists every device serial present in the database

#### Scenario: The row cap is signalled
- **WHEN** more than 500 configurations match the filters
- **THEN** the response contains 500 rows and a truncation flag set to
  true

### Requirement: Service exposes a paginated, filterable run listing
The system SHALL expose an HTTP operation that lists recorded runs newest
first (by start time, with the run ID as a deterministic tie-breaker),
returning per-run summaries and an opaque cursor for the next page. The
operation SHALL accept a page size (`limit`, default 50, maximum 200), an
opaque `cursor` from a previous response, and exact-match filters on device
serial, model asset ID, git commit SHA, git branch, git dirty flag, SUMD
driver version, BSP version, GPU clock, MIF clock, INT clock, prompt
SHA-256, exit status, and correctness result, combining multiple filters
conjunctively. Each summary SHALL include the run's ID, start and finish
times, repetition, device serial, git commit SHA, dirty flag, and branch,
SUMD driver version, BSP version, the referenced model's ID and original
name, exit status, correctness result, prefill throughput, decode
throughput (null when not recorded), and thermal-throttling flag. An
unrecognized exit status or correctness result filter value, a
non-positive or over-maximum `limit`, or an undecodable cursor SHALL be
rejected with the consistent error envelope's invalid-request code.

#### Scenario: Listing returns newest runs first
- **WHEN** a client lists runs with no filters
- **THEN** the response contains up to `limit` summaries ordered from the
  most recently started run to the least, and a next-page cursor if more
  runs exist

#### Scenario: Paging with the cursor neither skips nor repeats runs
- **WHEN** a client follows the returned cursor across successive pages
  until no cursor is returned
- **THEN** every run is returned exactly once in newest-first order, even
  if new runs are inserted between page requests

#### Scenario: Filters combine conjunctively
- **WHEN** a client lists runs filtered by a device serial and an exit
  status
- **THEN** only runs matching both are returned

#### Scenario: A full configuration key selects exactly one configuration's runs
- **WHEN** a client lists runs filtered by every field of a results row's
  configuration key
- **THEN** exactly the runs that contributed to that results row are
  returned

#### Scenario: Invalid filter or paging values are rejected
- **WHEN** a client supplies an exit status outside the stable enum, a
  `limit` of 0 or above the maximum, or a cursor not produced by the
  system
- **THEN** the system responds with the error envelope's invalid-request
  code rather than ignoring the value or failing internally

### Requirement: Single-run responses expose the complete recorded run
The single-run read operation SHALL return, in addition to the outcome and
artifact/model summaries it already returns: repetition, the command-line
argument array, the command line, input parameters, captured environment
variables, environment-allowlist version, collector version, device serial,
BSP version, driver version, device uptime in seconds, battery-charging
flag, initial and maximum temperatures in degrees Celsius, thermal-throttling
flag, GPU/MIF/INT clocks in MHz, git commit SHA, dirty flag, branch, commit
timestamp, and commit subject, executable SHA-256, prompt SHA-256, input
and output token counts, prefill throughput, decode throughput (null when
not recorded), and error summary. Every field already returned SHALL keep
its existing name and shape.

#### Scenario: A run's recorded measurements are readable over HTTP
- **WHEN** a client fetches a run that recorded clocks, temperatures, token
  counts, and throughput
- **THEN** the response contains each of those values exactly as recorded,
  with the JSON structured fields (command arguments, input parameters,
  environment variables) returned as JSON values rather than encoded
  strings

#### Scenario: Existing fields are unchanged
- **WHEN** a client written against the previous single-run response reads
  the new response
- **THEN** every field it relied on is present with the same name and type

### Requirement: Service optionally serves the built dashboard
When a dashboard output directory is configured, the system SHALL serve its
files as static assets from the site root, returning the directory's
`index.html` for the root path and for any path that is neither an existing
asset nor an API, health, or documentation route, so client-side routes
resolve on reload. API, health, OpenAPI, and Swagger UI routes SHALL take
precedence over static serving. When no directory is configured, the
system's behavior SHALL be unchanged. If a configured directory does not
exist or is not readable at startup, the system SHALL fail to start with an
error naming it, rather than silently serving nothing.

#### Scenario: Client-side route reloads resolve to the application shell
- **WHEN** a dashboard directory is configured and a client requests a
  path such as `/runs/<some-id>` that is not a file in that directory
- **THEN** the system responds with the directory's `index.html`

#### Scenario: API routes are unaffected by static serving
- **WHEN** a dashboard directory is configured and a client requests an API
  route, `/health`, `/openapi.json`, or `/docs`
- **THEN** the system responds exactly as it would without static serving
  configured

#### Scenario: Unconfigured static serving changes nothing
- **WHEN** no dashboard directory is configured and a client requests `/`
- **THEN** the system responds as it did before this capability existed

#### Scenario: A misconfigured dashboard directory fails startup
- **WHEN** the configured dashboard directory does not exist
- **THEN** the service exits at startup with an error naming that directory

### Requirement: Service accepts complete run records over HTTP
The system SHALL expose `POST /api/v1/runs` accepting a JSON body that
describes one complete run attempt: the client-assigned run ID, start and
finish times, repetition, command arguments and line, input parameters,
captured environment variables and allowlist version, collector version,
platform, device class, host identity and model name, the host snapshot
fields for that platform and class, git provenance, executable hash (null
when not preserved), the registered model asset ID, prompt hash, token
counts, prefill and decode throughput, exit status, correctness result,
optional artifact references (input, output, stdout, stderr, crash log),
an optional output preview, and an optional error summary. On success the
system SHALL store the run exactly once as an immutable row and respond
`201` with the same representation `GET /api/v1/runs/{id}` returns. The
run SHALL be visible to the listing and results operations immediately
after the response.

#### Scenario: A collector posts a succeeded repetition
- **WHEN** a client posts a run whose model asset and artifacts exist and
  whose fields satisfy the schema's rules
- **THEN** the service responds `201` with the stored run, and a subsequent
  `GET /api/v1/runs/{id}` and `GET /api/v1/runs` return it

#### Scenario: A collector posts a crashed repetition
- **WHEN** a client posts a run with exit status `crashed`, a null finish
  time, zero throughput, an error summary, and a crash-log artifact
- **THEN** the service stores it, and the results row for its configuration
  counts it as not succeeded without including it in any statistic

#### Scenario: An external phone posts without lab-only fields
- **WHEN** a client posts an `android`, `external` run with device model,
  build, kernel, SoC, and GPU but no BSP, SUMD, clocks, battery, or
  temperatures
- **THEN** the service accepts it and reads those fields back as null

### Requirement: Run creation validates references and snapshot rules before writing
The system SHALL reject a run submission, writing nothing, when: a
referenced model asset or artifact ID does not exist; the platform,
device class, exit status, or correctness result is not one of the
documented values; the snapshot fields do not match the platform and
device class (an `internal` Android run missing any lab field, a Linux run
carrying BSP, SUMD, or clock fields, or an external phone with only some
of BSP, SUMD, and the three clocks); a SHA-256 field is not 64 lowercase
hexadecimal characters; command arguments or input parameters are not the
required JSON shapes; a clock is not positive; a token count or throughput
is negative; or the repetition is negative. The rejection SHALL use the
consistent error envelope with code `invalid_request` and SHALL name the
offending field in `details`. A submission whose run ID already exists
SHALL be rejected with status `409` and code `conflict`, leaving the
existing run unchanged, so a client that lost a response can retry safely
and then confirm with `GET /api/v1/runs/{id}`.

#### Scenario: A reference to a missing artifact is rejected
- **WHEN** a client posts a run whose `stdout_artifact_id` does not exist
- **THEN** the service responds `400` with code `invalid_request`, `details`
  naming `stdout_artifact_id`, and no run is stored

#### Scenario: An internal device without its lab snapshot is rejected
- **WHEN** a client posts an `android`, `internal` run with no BSP version
- **THEN** the service responds `400` naming `bsp_version` and stores nothing

#### Scenario: A retried submission does not duplicate the run
- **WHEN** a client posts the same run ID twice
- **THEN** the second response is `409` with code `conflict`, and exactly
  one run with that ID exists

#### Scenario: An oversized output preview is bounded
- **WHEN** a client posts an output preview longer than the configured
  preview length
- **THEN** the stored preview is truncated to that length and the complete
  output remains available through the referenced output artifact

### Requirement: Service exposes model lookup by content hash
`GET /api/v1/models` SHALL accept an optional `sha256` query parameter and,
when present, return only the registered model asset with that hash (an
empty list when none matches), so a collector that knows a model file's
hash can obtain its asset ID without the backend reading the file.

#### Scenario: Looking up a registered model by hash
- **WHEN** a client requests `GET /api/v1/models?sha256=<hash of a
  registered model>`
- **THEN** the response lists exactly that asset

#### Scenario: An unregistered hash returns nothing
- **WHEN** a client requests the models list with a well-formed hash that
  no asset has
- **THEN** the response is an empty list with status `200`

#### Scenario: A malformed hash is rejected
- **WHEN** a client passes a `sha256` value that is not 64 lowercase
  hexadecimal characters
- **THEN** the service responds `400` with code `invalid_request`

### Requirement: Service streams change notifications as Server-Sent Events
The system SHALL expose `GET /api/v1/events` responding with
`text/event-stream` that stays open and emits one event whenever a run is
created (`run.created`, carrying the run's listing summary), an artifact is
stored (`artifact.created`, carrying its ID, kind, hash, and size), or a
model asset is registered (`model.registered`, carrying its ID, name, and
hash). Each event SHALL carry a monotonically increasing `id` for the
lifetime of the server process and a JSON `data` payload. The stream SHALL
send a comment keep-alive at least every 30 seconds while idle. Events are
change signals only: the stream SHALL NOT replay events missed while a
client was disconnected, and the REST operations remain the authoritative
source of state. Multiple concurrent subscribers SHALL each receive every
event; a subscriber too slow to keep up MAY be dropped and SHALL then
reconnect and re-fetch.

#### Scenario: A subscriber sees a run land
- **WHEN** a client is connected to the event stream and another client
  posts a run
- **THEN** the subscriber receives a `run.created` event whose data
  contains that run's ID and summary before any further event

#### Scenario: The stream stays alive while idle
- **WHEN** no change happens for 30 seconds
- **THEN** the subscriber has received at least one keep-alive comment and
  the connection remains open

#### Scenario: Events are not replayed
- **WHEN** a client connects after runs were created while it was
  disconnected
- **THEN** it receives no event for those runs and must re-fetch the REST
  operations to see them
