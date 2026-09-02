## Why

A collector running a benchmark on a phone or a Linux box has no way to
record its result: the only write paths are the Rust API and the log
importer, so every measurement so far has been hand-carried into the
database after the fact. The next step is Python scripts that drive the
benchmark and record each repetition as it lands, which needs an HTTP
write path for runs and, so the dashboard reflects a session while it
runs, a live change feed instead of reload-only polling.

## What Changes

- Add `POST /api/v1/runs`: a collector submits one complete, immutable run
  record per repetition after the process exits, referencing artifacts it
  uploaded through the existing artifact endpoint and a registered model
  asset. The body is the flat run shape the API already returns; the
  service validates platform and device-class rules, foreign references,
  JSON columns, hashes, and units, and answers with the stored run. The
  client supplies the run ID (UUID), so a retried submission is detected
  as a duplicate (`409`, new error code `conflict`) instead of creating a
  second row. No in-progress state, no progress or finalize endpoints:
  a run is written once, complete, exactly as the schema's immutability
  rule requires.
- Add a `sha256` filter to `GET /api/v1/models` so a collector can find
  the asset ID of a model it already has the hash of, without touching a
  path the backend can read.
- Add `GET /api/v1/events`: a Server-Sent Events stream of change
  notifications (`run.created`, `artifact.created`, `model.registered`)
  with keep-alives. It is a signal to re-fetch, not a data source; the
  REST endpoints stay authoritative and the stream is not replayed on
  reconnect.
- Dashboard: subscribe to the event stream and refresh the results and
  runs views when a run lands, with a visible live/disconnected indicator
  and no behaviour change when the stream is unavailable.
- Document the new operations in the generated OpenAPI contract (the
  `api-documentation` spec currently requires their absence), bump the
  API version, and add a short collector guide plus a dependency-free
  Python example that uploads artifacts and posts a run.
- No authentication: per the decision for this change, all endpoints
  stay open on the trusted LAN; a write token can be layered on later.

## Capabilities

### New Capabilities
<!-- none: both features are behaviours of the existing HTTP service -->

### Modified Capabilities
- `ingestion-service`: adds run creation over HTTP with validation and
  duplicate detection, model lookup by content hash, and the live event
  stream.
- `api-documentation`: the requirement that excludes run creation and
  the events endpoint from the document is reversed for the operations
  that now exist; the `events` tag becomes valid.
- `benchmark-dashboard`: results and runs pages refresh from the event
  stream and show its connection state.

## Impact

- **Backend**: new `POST /api/v1/runs` handler with a request type that
  mirrors the run response and maps onto `NewRun`/`HostState`; a `409
  conflict` error code; a broadcast channel in shared state that the run,
  artifact, and model write paths publish to; an SSE handler. No schema
  migration.
- **API**: additive. `api_version` moves to `1.3`. Clients of
  `GET /api/v1/models` keep working; the new filter is optional.
- **Dashboard**: an event subscription hook and a status indicator; the
  dev proxy must pass a streaming response through unbuffered.
- **Docs**: `docs/api.md` gap list shrinks to authentication only; a new
  `docs/collector.md` and `examples/post_run.py`.
- **Not in scope**: the Python benchmark runner itself, authentication,
  batch submission, and any run-update endpoint.
