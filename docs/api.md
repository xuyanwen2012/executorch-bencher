# HTTP API documentation

The backend's HTTP API is documented as a generated OpenAPI 3.x contract,
not a hand-maintained spec file - it is produced from the same Axum routes
and Rust types (`utoipa`) that serve the real requests, so the document
can't drift from the implementation the way a separately maintained YAML
file could.

## Where to look

- **Interactive docs (Swagger UI)**: `GET /docs` on the running server.
  "Try it out" works against whatever database/storage the server is
  currently configured with.
- **Raw OpenAPI document**: `GET /openapi.json` on the running server.
- **Checked-in copy**: [`openapi/openapi.json`](../openapi/openapi.json) in
  this repo - generated, not hand-edited (see below).

Neither `/docs` nor `/openapi.json` is versioned under `/api/v1/...`; every
other documented route is.

## Regenerating the checked-in contract

```sh
cargo run --bin gen-openapi
```

This writes `openapi/openapi.json` from the same route/schema definitions
the server uses - no database connection or running server required. Run it
after changing a route, request/response type, or schema annotation, and
commit the result alongside the code change.

A test (`tests/openapi_drift.rs`) asserts the checked-in file matches what
the server generates at runtime, so a forgotten regeneration fails CI (once
this repo has one) or a local `cargo test` run, rather than silently
drifting.

## Versioning and compatibility

`GET /api/v1/version` returns:

```json
{
  "api_version": "1.1",
  "server_version": "0.1.0",
  "minimum_runner_version": "0.1.0",
  "schema_version": 1
}
```

- `server_version` is derived from `Cargo.toml`'s package version
  (`CARGO_PKG_VERSION`) - never duplicated by hand.
- `api_version`, `schema_version`, and `minimum_runner_version` are
  hand-maintained constants (`src/version_api.rs`), bumped manually as the
  contract evolves. There is currently no Python runner in this repo to
  check compatibility against, so `minimum_runner_version` is a forward
  placeholder rather than an enforced gate today.
- There is no automatic runner/server compatibility check yet. When a
  Python runner client exists, it is expected to compare its own version
  against `minimum_runner_version` before proceeding.

## What's implemented vs. documented as a gap

The generated document only describes routes that actually exist. It
intentionally does **not** include (and reports as a gap rather than
inventing):

- Run finalize or progress updates (`POST /api/v1/runs/{run_id}/finalize`,
  `POST /api/v1/runs/{run_id}/progress`). A run is submitted once,
  complete, after the process exits; there is no in-progress state.
- Authentication (none exists in this service)

Everything currently implemented is documented: `GET /health`,
`GET /api/v1/version`, `POST /api/v1/runs` (a collector submits one
complete run record; see `collector.md`), `GET /api/v1/results` (grouped
per-configuration statistics with filter facets), `GET /api/v1/runs`
(paginated, filterable listing), `GET /api/v1/runs/{id}` (the complete
recorded run, including device state, clocks, temperatures, throughput,
token counts, build identity, and git metadata), `GET /api/v1/events`
(live change notifications), artifact upload/metadata/content/download,
and model register/list (with a `sha256` lookup filter)/get/verify.

`api_version` moved from `1.0` to `1.1` when the results and listing
operations and the expanded run fields were added (additive; no existing
field changed name or shape), and to `1.2` when runs gained a `platform`
(`android` | `linux`) with Linux host fields (`host_os`, `host_kernel`,
`host_cpu_model`, `host_cpu_count`, `host_memory_bytes`,
`host_accelerator`, `host_accelerator_driver`). That change also made the
Android-only fields (`bsp_version`, `sumd_driver_version`,
`battery_charging`, the temperatures, `thermal_throttling`, and the three
clocks) and `executable_sha256` nullable, so clients that assumed them
non-null must check for `null`. `platform`, `device_class`, and `host_accelerator`
are filters on the listing and results operations, and facets list
`platforms`, `device_classes`, and `host_accelerators`. Runs also carry
`device_class` (`internal` lab device with the full Android snapshot, or
`external` retail phone / Linux box with only what it can report) and
`device_model`; the `host_*` fields are populated on Android too when the
phone reported them.

### Static dashboard serving and route precedence

When `DASHBOARD_DIST` is set, the built dashboard is served from `/` with
a single-page-app fallback: any path that no API, health, or documentation
route matches and that is not a file in the directory returns
`index.html` with status 200. Registered routes always take precedence, so
an unknown *run ID* under `/api/v1/runs/{id}` is still an API 404, but an
unregistered path such as `/api/v1/does-not-exist` reaches the fallback
and returns the application shell. Register new API routes on the router
to make them reachable; nothing under `/api/` is served from the
dashboard directory by design.

## Generating typed clients

The document is the source for typed clients, not just human-readable
docs. The dashboard in `dashboard/` is the first such client: `bun run
generate-api` runs `openapi-typescript` over `openapi/openapi.json` to
produce `dashboard/src/api/schema.d.ts` (checked in), and
`openapi-fetch` provides the typed calls. `bun run check` fails when the
generated file is stale, mirroring the Rust drift test.

A Python client can be generated the same way once a runner package
exists:

```sh
pip install openapi-python-client
openapi-python-client generate --path openapi/openapi.json
```

Wrap generated clients in a small application-specific API rather than
editing generated code by hand - regenerate and re-wrap instead.

## The event stream is a signal, not state

`GET /api/v1/events` is a continuous `text/event-stream`. OpenAPI has no
first-class construct for that, so the operation is documented as a
`text/event-stream` string response whose description names the events,
and each event's `data` payload has a component schema
(`RunCreatedEvent`, `ArtifactCreatedEvent`, `ModelRegisteredEvent`) that
generated clients can use. Events: `run.created`, `artifact.created`,
`model.registered`; each carries a process-scoped increasing `id` and a
JSON `data` payload, and a comment keep-alive is sent while idle
(`EVENTS_KEEP_ALIVE_SECONDS`, default 15).

Unlike every other endpoint in this document, the stream does not return
"the current state": it tells a client *when* to re-fetch the
authoritative state from the REST endpoints above. Nothing is persisted
or replayed - a client that reconnects receives no events for what it
missed and should simply re-fetch. A subscriber that falls too far behind
is disconnected and should reconnect. The dashboard uses the stream this
way, coalescing bursts into one refresh.

`api_version` moved to `1.3` when run creation, the model hash lookup,
and the event stream were added (additive).
