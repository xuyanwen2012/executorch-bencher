## MODIFIED Requirements

### Requirement: The document excludes unimplemented operations and unexposed fields
The generated OpenAPI document SHALL NOT include a path, tag, or schema
component for an operation that has no corresponding implemented HTTP
route — including run creation, run finalize, run progress, and the
live-events endpoint, none of which exist yet. Run listing
(`GET /api/v1/runs`) and grouped results (`GET /api/v1/results`) are now
implemented and SHALL be documented. The document SHALL NOT describe a
field on a response schema unless the system's handler actually returns
that field; now that `GET /api/v1/runs/{id}` returns the run's device
state, clocks, temperatures, uptime, throughput, token counts, build
identity, and git metadata, the run response schema SHALL document exactly
that returned field set, and no more.

#### Scenario: Unimplemented run-lifecycle operations are absent
- **WHEN** a client inspects the document's paths
- **THEN** it contains no `POST /api/v1/runs`, no
  `POST /api/v1/runs/{run_id}/finalize`, no
  `POST /api/v1/runs/{run_id}/progress`, and no `GET /api/v1/events`

#### Scenario: The implemented listing and results operations are present
- **WHEN** a client inspects the document's paths
- **THEN** it contains `GET /api/v1/runs` with its `limit`, `cursor`, and
  filter query parameters and its summary-list response schema, and
  `GET /api/v1/results` with its filter parameters and its rows-plus-facets
  response schema

#### Scenario: Unimplemented tags are absent
- **WHEN** a client inspects the document's tag list
- **THEN** it contains no `events` or `analysis` tag, since no operation
  uses them

#### Scenario: Unexposed run fields are absent from the run response schema
- **WHEN** a client inspects the schema the document associates with
  `GET /api/v1/runs/{id}`'s response
- **THEN** it lists exactly the fields the handler serializes - now
  including the clock, temperature, uptime, throughput, token-count,
  build-identity, and git metadata fields - and no field the handler does
  not return

## ADDED Requirements

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
