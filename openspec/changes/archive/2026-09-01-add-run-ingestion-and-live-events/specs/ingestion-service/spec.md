## ADDED Requirements

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

## MODIFIED Requirements

### Requirement: HTTP error responses use a consistent JSON envelope
Every failed or rejected HTTP request SHALL respond with a JSON body of
the form `{ "error": { "code": <stable string>, "message": <human text>,
"details": <optional object>, "request_id": <optional string> } }`, with
`code` drawn from a documented set that includes at least
`invalid_request`, `not_found`, `artifact_file_missing`,
`payload_too_large`, `conflict`, `not_implemented`, and `internal_error`,
and an HTTP status consistent with the code (`400`, `404`, `404`, `413`,
`409`, `501`, `500` respectively). Clients SHALL match on `code`, never on
`message`.

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
