## ADDED Requirements

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
