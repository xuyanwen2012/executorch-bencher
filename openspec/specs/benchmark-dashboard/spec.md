# benchmark-dashboard Specification

## Purpose

Provides the browser dashboard through which engineers view benchmark
results recorded by the service: a per-configuration results table to
eyeball, a filterable list of runs for diagnosis, and the complete record
of a single run with the artifacts and model it references.

## Requirements

### Requirement: Dashboard is a Bun-toolchain single-page application in this repository
The dashboard SHALL live in a `dashboard/` package inside this repository
and SHALL use Bun as its package manager, development server, bundler, and
test runner, with no Node.js-based build toolchain required. `bun install`
followed by `bun run dev` SHALL start a development server; `bun run build`
SHALL produce a self-contained static output directory (an `index.html`
plus hashed assets, with the stylesheet fully compiled) suitable for
serving by any static file server or by the backend; `bun run check` SHALL
type-check the source; and `bun test` SHALL run the dashboard's unit
tests. All four SHALL succeed on a fresh checkout given only Bun and
network access for dependency installation.

#### Scenario: Fresh checkout builds without a Node toolchain
- **WHEN** a developer with Bun installed (and no Node.js on the path) runs
  `bun install` and `bun run build` in `dashboard/`
- **THEN** a static output directory containing `index.html` and the
  bundled assets is produced with exit status 0, and the emitted
  stylesheet contains no unprocessed Tailwind directives

#### Scenario: Development server proxies API requests to the backend
- **WHEN** the development server is running and the dashboard requests any
  path under `/api/` or `/health`
- **THEN** the request is forwarded to the configured backend base URL
  (default `http://127.0.0.1:3000`) and its response returned with status,
  headers, and streamed body unchanged, so the browser only ever talks to
  the development server's origin

#### Scenario: Type-check and tests pass on a fresh checkout
- **WHEN** a developer runs `bun run check` and `bun test` in `dashboard/`
- **THEN** both exit with status 0

### Requirement: Dashboard API access is typed from the generated OpenAPI contract
The dashboard SHALL access the backend only through a typed client whose
request and response types are generated from the checked-in
`openapi/openapi.json`, via a `bun run generate-api` command. The generated
type file SHALL be checked in, and a check SHALL fail when the checked-in
generated types no longer match what the current `openapi/openapi.json`
would generate, so a backend contract change cannot silently leave the
dashboard typed against a stale contract.

#### Scenario: Regenerating after a contract change updates the client types
- **WHEN** `openapi/openapi.json` changes and `bun run generate-api` is run
- **THEN** the checked-in generated type file reflects the changed contract

#### Scenario: Stale generated types are detected
- **WHEN** `openapi/openapi.json` has changed but `bun run generate-api`
  has not been re-run
- **THEN** the dashboard's check command fails, identifying the generated
  types as out of date, rather than passing silently

### Requirement: Results page shows one row per benchmark configuration
The dashboard SHALL open on a results page showing one row per benchmark
configuration as returned by the grouped results operation, newest commit
first. Each row SHALL show the commit (short SHA, branch, subject, and
commit time when recorded, with a visible dirty marker when the working
tree was dirty), the model (its `.pte` filename without extension), the
device serial, the SUMD driver version, the BSP version, the GPU/MIF/INT
clocks in MHz, the input token count, the succeeded-run count, prefill
throughput as median with min–max in tokens per second, decode throughput
likewise, a not-succeeded count and a correctness-failed count when
nonzero, a throttled count when nonzero, and the latest run time. A
throughput statistic the backend reports as null SHALL be shown as an
explicit absent marker, never as zero. The page SHALL offer a control to
choose which throughput (prefill or decode) is emphasized, defaulting to
prefill, and SHALL offer filters on device, model, branch, SUMD driver,
BSP, and dirty flag whose options come from the response's `facets`. Active
filters SHALL be reflected in the page URL. Each row SHALL link to the runs
page pre-filtered to exactly that row's configuration key.

#### Scenario: Repetitions appear as one row
- **WHEN** several runs share one configuration key
- **THEN** the results page shows a single row for them with the median,
  minimum, maximum, and count reported by the backend

#### Scenario: Filtering and sharing a filtered view
- **WHEN** a user filters by one device and one model
- **THEN** only matching rows are shown and the page URL encodes both
  filters such that opening that URL restores the same view

#### Scenario: A row links to its runs
- **WHEN** a user activates a results row
- **THEN** the runs page opens filtered to that row's device, model,
  commit, dirty flag, driver, BSP, clocks, and prompt, listing exactly the
  runs that contributed to the row

#### Scenario: A configuration with no succeeded runs
- **WHEN** a row's throughput statistics are null because every run failed
- **THEN** the row shows absent markers for the statistics and the failure
  count, rather than zeros

#### Scenario: Truncated results are signalled
- **WHEN** the backend reports the row cap was reached
- **THEN** the page shows a notice that not all configurations are listed
  and suggests narrowing the filters

#### Scenario: No results
- **WHEN** no runs exist or none match the active filters
- **THEN** the page shows an explicit empty state distinguishing "no runs
  recorded yet" from "no configurations match these filters"

### Requirement: Results page collapses columns that do not vary
Among the configuration-key columns (model, device, driver, BSP, each
clock, input token count), the results page SHALL hide any column whose
value is identical across every visible row and SHALL list those shared
values in a "shared configuration" line above the table, so the table
shows only the dimensions that differ. A toggle SHALL show every column
regardless.

#### Scenario: A constant dimension is collapsed
- **WHEN** every visible row has the same BSP version and the same three
  clocks
- **THEN** the BSP and clock columns are hidden and the shared line reads
  their values, while columns that differ (for example driver) remain

#### Scenario: A dimension reappears when it varies
- **WHEN** a filter change makes rows with two different BSP versions
  visible
- **THEN** the BSP column appears in the table and leaves the shared line

#### Scenario: Show-all toggle
- **WHEN** the user enables the show-all toggle
- **THEN** every configuration-key column is shown regardless of variation

### Requirement: Runs page lists runs newest first with filters and paging
The dashboard SHALL provide a runs page showing runs newest first, one row
per run, with at least: start time, device serial, model name, short git
commit with a dirty marker, branch, SUMD driver, repetition, exit status,
correctness result, prefill throughput, and decode throughput. The page
SHALL offer filters for device serial, model, git commit, branch, dirty
flag, SUMD driver, BSP, exit status, and correctness result, SHALL accept
the full configuration-key filter set via the URL (as linked from a results
row), SHALL reflect active filters in the URL, and SHALL let the user load
further pages until no more runs match. A decode throughput the run did not
record SHALL be displayed as absent, never as zero.

#### Scenario: Newest runs appear first
- **WHEN** a user opens the runs page with runs recorded at different start
  times
- **THEN** the list shows the most recently started run at the top

#### Scenario: Filtering narrows the list and updates the URL
- **WHEN** a user filters by a device serial and an exit status
- **THEN** only runs matching both are shown, and the page URL encodes both
  filters such that opening that URL restores the same filtered view

#### Scenario: Paging through a long history
- **WHEN** more runs match than fit in one page
- **THEN** the user can request the next page, and the newly loaded runs
  continue the newest-first order without duplicating or skipping runs

#### Scenario: A run without decode throughput
- **WHEN** a listed run recorded no decode throughput
- **THEN** the decode column shows an explicit "not recorded" marker rather
  than `0`

#### Scenario: No runs match
- **WHEN** no runs exist or none match the active filters
- **THEN** the list shows an explicit empty state distinguishing "no runs
  recorded yet" from "no runs match these filters"

### Requirement: Run detail view shows the complete run record
Selecting a run SHALL open a detail view at a URL containing the run's ID,
showing every field the single-run endpoint returns, grouped as: run
metadata (timing, repetition, command line and arguments, input parameters,
captured environment variables, collector and allowlist versions), device
state (serial, BSP and driver versions, uptime, battery charging,
temperatures, thermal throttling), performance configuration (GPU, MIF, and
INT clocks with their MHz unit), build and workload identity (git commit,
dirty flag, branch, commit time and subject, executable SHA-256, prompt
SHA-256, token counts), and results (exit status, correctness result,
prefill and decode throughput, output preview, error summary). The
referenced model SHALL be shown with its name, SHA-256, and availability.
Temperatures SHALL be labelled in degrees Celsius and throughput in tokens
per second.

#### Scenario: Opening a run by URL
- **WHEN** a user navigates directly to the detail URL for an existing run
- **THEN** the view renders that run's complete record without first
  visiting the list

#### Scenario: Optional fields are shown as absent, not blank
- **WHEN** a run has no finish time, no command line, no output preview,
  no error summary, or no git metadata
- **THEN** each such field is displayed with an explicit absent marker
  rather than an empty cell or `null`

#### Scenario: Unknown run ID
- **WHEN** a user navigates to a detail URL whose run ID does not exist
- **THEN** the view shows a "run not found" message and a link back to the
  runs page, rather than a blank page or an unhandled error

### Requirement: Run detail view exposes each attached artifact
For each artifact a run references (input prompt, output, stdout, stderr,
crash log), the detail view SHALL show its kind, original filename, size,
media type, compression, and availability, and SHALL offer a download
link. For an available artifact with a text media type at or below a
bounded preview size, the view SHALL also offer inline viewing of its
content. An artifact whose file is reported unavailable SHALL be shown as
unavailable with no active view or download link.

#### Scenario: Downloading an available artifact
- **WHEN** a user activates the download link for an available artifact
- **THEN** the browser downloads the artifact's decompressed content under
  a safe filename, served by the backend

#### Scenario: Viewing a small text artifact inline
- **WHEN** a user opens the inline view of an available `text/*` artifact
  within the preview size bound
- **THEN** its decompressed content is shown in the page

#### Scenario: A large or binary artifact is download-only
- **WHEN** an artifact exceeds the preview size bound or has a non-text
  media type
- **THEN** the view offers download only, without attempting to render the
  content inline

#### Scenario: An artifact whose file is missing
- **WHEN** a run references an artifact the backend reports as unavailable
- **THEN** the artifact is listed with an "unavailable" marker and no
  active view or download link

### Requirement: Dashboard presents times in local time with UTC available
Tables SHALL display timestamps in the browser's local time zone, and the
full UTC RFC 3339 timestamp SHALL be available on hover. The run detail
view SHALL display both the local rendering and the full UTC timestamp.

#### Scenario: Hovering a table timestamp
- **WHEN** a user hovers a start time in the results or runs table
- **THEN** the full UTC timestamp is shown

#### Scenario: Detail view shows both
- **WHEN** a user views a run's start time on the detail page
- **THEN** both the local rendering and the UTC timestamp are visible

### Requirement: Dashboard surfaces backend errors and unreachability clearly
The dashboard SHALL show a loading state while a request is in flight, and
on failure SHALL show the backend error envelope's `code` and `message`
when one is returned, or a "backend unreachable" message when no response
is received, with a way to retry. It SHALL NOT render a partially populated
view as if the request had succeeded.

#### Scenario: Backend returns an error envelope
- **WHEN** a request fails with the backend's JSON error envelope
- **THEN** the dashboard displays the envelope's `code` and `message` and
  offers a retry

#### Scenario: Backend is unreachable
- **WHEN** a request receives no HTTP response
- **THEN** the dashboard displays a message that the backend could not be
  reached and offers a retry

### Requirement: Dashboard refreshes live from the event stream
The dashboard SHALL subscribe to the backend's event stream while the
results or runs page is open and SHALL re-fetch the data those pages show
when a `run.created` event arrives, coalescing bursts so that many runs
landing within a short interval cause one refresh. It SHALL show whether
the stream is connected, and when it is not (unsupported, unreachable, or
dropped) SHALL keep working exactly as before with manual reload, retrying
the subscription in the background. A live refresh SHALL NOT discard the
user's filters, paging position, or scroll position.

#### Scenario: A run lands while the results page is open
- **WHEN** a collector posts a run for a configuration currently listed
- **THEN** within a few seconds the row's statistics and counts update
  without the user reloading, and the active filters are unchanged

#### Scenario: A burst of repetitions
- **WHEN** eighteen runs are posted within ten seconds
- **THEN** the page refreshes a small number of times, not eighteen, and
  ends up showing all eighteen

#### Scenario: The stream is unavailable
- **WHEN** the event stream cannot be opened or drops
- **THEN** the page shows a "live updates off" state, all data still loads
  on navigation and reload, and the dashboard reconnects without user
  action once the backend is reachable again
