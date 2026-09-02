## Purpose

Provides a generated, authoritative OpenAPI contract and interactive
documentation for the backend's HTTP API, so the Python benchmark runner,
the future TypeScript dashboard, and human developers all work against one
accurate, machine-readable description of the real implemented endpoints.

## ADDED Requirements

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
today — see api-documentation's "document excludes unimplemented
operations and unexposed fields" requirement for fields that exist in the
domain layer but are not returned by any endpoint.

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
route — including run creation, run finalize, run progress, run listing,
and the live-events endpoint, none of which exist yet. The document SHALL
NOT describe a field on a response schema unless the system's handler
actually returns that field — in particular, the run response schema SHALL
NOT claim to return device state, clocks, temperatures, uptime,
prefill/decode speed, token counts, or build-identity fields that the `Run`
domain type carries internally but that `GET /api/v1/runs/{run_id}` does
not currently return.

#### Scenario: Unimplemented run-lifecycle operations are absent
- **WHEN** a client inspects the document's paths
- **THEN** it contains no `POST /api/v1/runs`, no
  `POST /api/v1/runs/{run_id}/finalize`, no
  `POST /api/v1/runs/{run_id}/progress`, and no `GET /api/v1/runs`

#### Scenario: Unimplemented tags are absent
- **WHEN** a client inspects the document's tag list
- **THEN** it contains no `events` or `analysis` tag, since no operation
  uses them

#### Scenario: Unexposed run fields are absent from the run response schema
- **WHEN** a client inspects the schema the document associates with
  `GET /api/v1/runs/{run_id}`'s response
- **THEN** it lists only the fields `RunResponse` actually serializes, and
  no clock, temperature, uptime, speed, token-count, or build-identity
  field the handler does not return
