## ADDED Requirements

### Requirement: HTTP error responses use a consistent JSON envelope
The system SHALL respond to every rejected or failed request across its
HTTP operations with a JSON body of the form
`{"error": {"code", "message", "details", "request_id"}}`, using a stable,
machine-readable `code` drawn from the failure's category, rather than a
bare string or a shape that varies by endpoint. The system SHALL NOT
include SQLite error text, SQL statements, absolute filesystem paths,
secrets, or stack traces in this response.

#### Scenario: An oversized artifact upload returns the envelope
- **WHEN** a client uploads content exceeding the configured maximum
  artifact upload size
- **THEN** the system rejects the request with a JSON body matching the
  consistent error envelope, with a `code` identifying the failure as
  exceeding the size limit

#### Scenario: A request for an unknown resource returns the envelope
- **WHEN** a client requests a run, artifact, or model by an ID that does
  not exist
- **THEN** the system responds with a JSON body matching the consistent
  error envelope, with a `code` identifying the resource as not found

#### Scenario: An artifact whose file is missing returns the envelope
- **WHEN** a client requests the content or download of an artifact whose
  database record exists but whose file is absent from disk
- **THEN** the system responds with a JSON body matching the consistent
  error envelope, with a `code` distinguishing this case from a generic
  not-found or internal error

#### Scenario: An invalid artifact kind returns the envelope
- **WHEN** a client uploads an artifact with a `kind` value outside the
  system's recognized artifact kinds
- **THEN** the system rejects the request with a JSON body matching the
  consistent error envelope, with a `code` identifying the request as
  invalid
