# api-documentation Specification

## Purpose

Provides a generated, authoritative OpenAPI contract and interactive
documentation for the backend's HTTP API, so the Python benchmark runner,
the future TypeScript dashboard, and human developers all work against one
accurate, machine-readable description of the real implemented endpoints.

## Requirements

### Requirement: System exposes a generated OpenAPI document
The system SHALL expose `GET /openapi.json` returning a complete, valid
OpenAPI 3.x document generated from the actual Axum routes and Rust
request/response types rather than a hand-maintained file, covering every
currently implemented `/api/v1/...` operation plus `/health`.

#### Scenario: Fetching the OpenAPI document
- **WHEN** a client requests `GET /openapi.json`
- **THEN** the system responds with a well-formed OpenAPI 3.x JSON document
  describing every currently implemented HTTP operation

#### Scenario: Document metadata avoids marketing language
- **WHEN** a client inspects the document's title and description
- **THEN** they describe the service factually as an internal Android LLM
  benchmark collection and analysis API, without promotional language

### Requirement: System exposes interactive API documentation
The system SHALL expose `GET /docs` serving an interactive Swagger-style UI
backed by the document at `/openapi.json`. Neither `/docs` nor
`/openapi.json` is versioned under `/api/v1/...`.

#### Scenario: Interactive docs load and reference the OpenAPI document
- **WHEN** a client requests `GET /docs`
- **THEN** the system responds with an interactive documentation page that
  fetches its schema from `/openapi.json`

### Requirement: System exposes version and compatibility information
The system SHALL expose `GET /api/v1/version` returning `api_version`,
`schema_version`, and `minimum_runner_version` as maintained constants, and
`server_version` derived from the crate's Cargo package version rather than
duplicated by hand.

#### Scenario: Version endpoint reflects the built package version
- **WHEN** a client requests `GET /api/v1/version`
- **THEN** the response's `server_version` equals the Cargo package version
  the running binary was built from, alongside the `api_version`,
  `schema_version`, and `minimum_runner_version` constants

### Requirement: Documented schemas describe enums, units, and nullability accurately
For every field backed by one of the system's stable enums (exit status,
correctness result, artifact kind, model storage mode), the generated
schema SHALL enumerate its exact stable snake_case values rather than
describing it as an unconstrained string. For every SHA-256 field the
system currently exposes, the schema SHALL describe it as a lowercase
64-character hexadecimal digest. A field the system can legitimately omit
or leave null (for example, a run's `output_preview`, `finished_at`, or an
optional artifact/model reference) SHALL be marked nullable rather than
required. This requirement covers only fields the API actually returns
today — see "The document excludes unimplemented operations and unexposed
fields" for fields that exist in the domain layer but are not returned by
any endpoint.

#### Scenario: Exit status enumerates its exact stable values
- **WHEN** a client inspects the schema for a run's exit status field
- **THEN** the schema lists exactly `succeeded`, `crashed`, `timed_out`,
  `cancelled`, and `infrastructure_error`, and no other values

#### Scenario: SHA-256 fields are documented and constrained
- **WHEN** a client inspects the schema for an artifact's or model asset's
  `sha256` field
- **THEN** the description states it is a lowercase 64-character
  hexadecimal SHA-256 digest

#### Scenario: Optional response fields remain nullable
- **WHEN** a client inspects the schema for a run's `output_preview` or
  `finished_at`, or for an optional artifact/model reference on a run
- **THEN** the corresponding fields are marked nullable rather than always
  required

### Requirement: Checked-in OpenAPI contract stays in sync with the runtime document
The system SHALL provide one deterministic command that regenerates the
checked-in `openapi/openapi.json` from the same route and type definitions
the running server uses, and a test that fails when the checked-in file
does not match the document the running server currently serves at
`/openapi.json`.

#### Scenario: Regenerating the contract reflects the current implementation
- **WHEN** the regeneration command is run after a route or schema changes
- **THEN** the checked-in `openapi/openapi.json` reflects that change

#### Scenario: A stale checked-in contract is detected
- **WHEN** the checked-in `openapi/openapi.json` no longer matches the
  document the server generates at runtime
- **THEN** the drift-check test fails rather than silently passing

### Requirement: The document excludes unimplemented operations and unexposed fields
The generated OpenAPI document SHALL NOT include a path, tag, or schema
component for an operation that has no corresponding implemented HTTP
route. Run creation (`POST /api/v1/runs`), the live-events stream
(`GET /api/v1/events`), run listing, grouped results, and the model
lookup filter are implemented and SHALL be documented; run finalize and
run progress operations do not exist and SHALL remain absent. The
document SHALL NOT describe a field on a response schema unless the
system's handler actually returns that field, and the run creation
request schema SHALL list exactly the fields the handler accepts, with
the same names, units, nullability, and enumerations as the run response
schema. The events operation SHALL be documented as a `text/event-stream`
response whose description names each event type and references a
component schema for each event's `data` payload, since OpenAPI has no
native streaming-event construct.

#### Scenario: Implemented write and stream operations are present
- **WHEN** a client inspects the document's paths
- **THEN** it contains `POST /api/v1/runs` with a request body schema and
  `201`, `400`, and `409` responses, and `GET /api/v1/events` with a
  `text/event-stream` response

#### Scenario: Unimplemented run-lifecycle operations are absent
- **WHEN** a client inspects the document's paths
- **THEN** it contains no `POST /api/v1/runs/{run_id}/finalize` and no
  `POST /api/v1/runs/{run_id}/progress`

#### Scenario: The implemented listing and results operations are present
- **WHEN** a client inspects the document's paths
- **THEN** it contains `GET /api/v1/runs` with its `limit`, `cursor`, and
  filter query parameters and its summary-list response schema, and
  `GET /api/v1/results` with its filter parameters and its rows-plus-facets
  response schema

#### Scenario: Unimplemented tags are absent
- **WHEN** a client inspects the document's tag list
- **THEN** it contains an `events` tag used by the stream operation and no
  `analysis` tag, since no operation uses one

#### Scenario: Request and response run schemas agree
- **WHEN** a client compares the run creation request schema with the run
  response schema
- **THEN** every field the request accepts appears in the response with the
  same name and enumeration, and the response's derived fields (artifact
  views, model summary) are the only additions

#### Scenario: Unexposed run fields are absent from the run response schema
- **WHEN** a client inspects the schema the document associates with
  `GET /api/v1/runs/{id}`'s response
- **THEN** it lists exactly the fields the handler serializes and no field
  the handler does not return

### Requirement: Documented numeric run fields state their units
For every numeric field the API returns whose value carries a physical
unit, in the run response, run summary, and results row schemas, the
generated schema SHALL state that unit in the field's description: clocks
in MHz, temperatures in degrees Celsius, uptime in seconds, and throughput
statistics in tokens per second. Fields that can be absent (decode
throughput and its statistics, throughput statistics of a configuration
with no succeeded runs, finish time, error summary, command line, output
preview, git branch, commit timestamp, commit subject) SHALL be marked
nullable.

#### Scenario: Clock and temperature fields document their units
- **WHEN** a client inspects the run response schema's GPU clock and
  initial temperature fields
- **THEN** their descriptions state MHz and degrees Celsius respectively

#### Scenario: Decode throughput is nullable
- **WHEN** a client inspects the run summary and run response schemas'
  decode throughput field and the results row's decode statistics
- **THEN** they are marked nullable and their descriptions state tokens
  per second
