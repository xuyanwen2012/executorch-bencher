## MODIFIED Requirements

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
