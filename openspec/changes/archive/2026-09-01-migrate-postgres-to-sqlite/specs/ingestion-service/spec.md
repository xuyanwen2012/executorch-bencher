## MODIFIED Requirements

### Requirement: Service starts with a database connection
The system SHALL, on startup, open a connection pool to the local SQLite
database file using externally supplied configuration, creating the file and
its containing directory if they do not already exist, and SHALL fail to
start with a clear error if the database file cannot be opened or created.

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

## ADDED Requirements

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
