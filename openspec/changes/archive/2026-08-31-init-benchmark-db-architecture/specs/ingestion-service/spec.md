## Purpose

Provides the running HTTP service that will host the benchmark ingestion and
dashboard APIs, established here as a minimal, operable skeleton that later
changes extend with actual endpoints.

## ADDED Requirements

### Requirement: Service starts with a database connection
The system SHALL, on startup, establish a connection pool to the
authoritative relational database using externally supplied configuration,
and SHALL fail to start with a clear error if a connection cannot be
established.

#### Scenario: Database is unreachable at startup
- **WHEN** the service is started and the configured database is unreachable
- **THEN** the service exits with a non-zero status and an error message
  identifying the connection failure, rather than starting in a broken state

#### Scenario: Database is reachable at startup
- **WHEN** the service is started and the configured database is reachable
- **THEN** the service establishes a connection pool and proceeds to accept
  HTTP requests

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
