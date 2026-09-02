## Context

See proposal.md for motivation. Constraints that shape the approach:

- Runs are immutable rows and the schema already encodes every rule a
  submission must satisfy (platform/device-class CHECK, hash lengths,
  JSON validity, non-negative counts). `runs::insert_run` plus
  `domain::validate_*` are the single write path today and the importer
  proves the shape works for real collectors.
- The API already returns a flat run representation. A request type that
  mirrors it keeps the OpenAPI document self-consistent and lets a Python
  client be generated or hand-written against one shape.
- The dashboard is a single-page app talking to the same origin through a
  Bun dev proxy in development; anything streamed must pass through that
  proxy unbuffered.
- Everything runs on one workstation plus LAN hosts; there is one backend
  process, so an in-process broadcast is enough for events.

## Goals / Non-Goals

**Goals:**
- One HTTP call records one complete repetition, validated as strictly as
  the Rust API, with a safe retry story.
- The dashboard reflects a running session within seconds, without the
  event stream becoming a second source of truth.
- A collector can be written in Python with only the standard library.

**Non-Goals:**
- In-progress runs, progress updates, or finalize (rejected in favour of
  single-shot records; see proposal).
- Event replay, persistence, or filtering; multi-process fan-out.
- Authentication (explicitly deferred).
- Batch submission; a collector posts one run per repetition.

## Decisions

### Request shape mirrors the run response, converted server-side to `NewRun`

`CreateRunRequest` is a `Deserialize + ToSchema` struct with the same
field names, units, and enumerations as `RunResponse`, minus the derived
parts (artifact views, model summary) and plus the artifact IDs. The
handler converts it into `NewRun` by: parsing enums through the existing
`TryFrom<&str>`; canonicalising `command_args`, `input_parameters`, and
`env_vars` through `domain::validate_*`; building `HostState` from
`platform` and the flat snapshot fields (Android: description fields plus
`AndroidLabConfig` only when BSP, SUMD, and the three clocks are all
present, error if some are; Linux: required description fields, error if
any lab field is present); checking that every referenced artifact and
the model asset exist; and truncating `output_preview` to the configured
length. Validation errors carry `details: {"field": name}`.

Alternative considered: a nested `host: {android: {...}} | {linux: {...}}`
body. It is closer to the Rust enum but diverges from the response shape,
so a client would need two models for one entity.

### Duplicate detection by client-assigned ID, `409 conflict`

The client generates the run's UUID (v7 recommended so it sorts by time).
Insert is attempted directly; a primary-key violation maps to `409` with
the new `conflict` code. This gives at-least-once clients a safe retry
without an idempotency-key table. Alternative: server-generated IDs with
an `Idempotency-Key` header - more machinery for the same outcome.

### Validation precedes the transaction; existence checks are explicit queries

Referenced IDs are looked up before `insert_run` so the error names the
field rather than surfacing a foreign-key message. The insert itself
remains the last step, so a rejected submission never leaves a row. The
CHECK constraints stay as a second line of defence; a CHECK failure that
slips through validation is reported as `internal_error`, and a test
asserts the validator catches every rule the CHECK encodes.

### Events: in-process `tokio::sync::broadcast`, SSE via axum's `Sse`

`AppState` gains an `events: broadcast::Sender<Event>`. The run, artifact,
and model write paths publish after their database commit (a subscriber
that re-fetches on receipt must see the row). `GET /api/v1/events` maps a
`BroadcastStream` into `axum::response::sse::Event` values with a
process-scoped sequence as `id`, JSON `data`, and a `KeepAlive` every
15 seconds (spec minimum 30). A lagging receiver returns `Lagged`; the
handler ends the stream so the client's `EventSource` reconnects and the
dashboard re-fetches. No filters and no replay keep the contract small
and honest: the stream is a poke, REST is the state. Alternatives:
WebSockets (bidirectional, unnecessary) and long-polling (more requests,
no simpler).

### Event publication lives in the API layer, not the storage modules

`runs::insert_run`, `artifact_store::store_artifact`, and
`ModelStorage::register` stay free of HTTP concerns; the handlers publish.
The importer and seeder therefore do not emit events, which is fine: they
are offline tools.

### Dashboard subscription: one hook, coalesced invalidation

A `useLiveEvents` hook opens an `EventSource` on `/api/v1/events`, keeps a
connection state, and on `run.created` schedules
`queryClient.invalidateQueries` for `results` and `runs` behind a
~500 ms trailing debounce, so a burst refreshes once. TanStack Query's
refetch keeps existing data on screen until the new page arrives, so
filters, paging, and scroll are preserved. `EventSource` reconnects on
its own; the indicator reflects `readyState`. The Bun dev proxy already
forwards the upstream body as a stream; a test hits the proxy with an
open stream to confirm no buffering.

### OpenAPI for the stream

utoipa cannot describe SSE. The operation is documented with a
`text/event-stream` response of type string and a description listing
the event names; each payload gets a `ToSchema` component
(`RunCreatedEvent`, `ArtifactCreatedEvent`, `ModelRegisteredEvent`)
referenced from that description so generated clients still get typed
payloads. The contract tests assert the operation, the tag, and the
component schemas are present.

### Model lookup by hash

`GET /api/v1/models?sha256=` reuses `model_registry::find_by_sha256`. It
avoids a second registration endpoint and matches how the importer
already resolves models. A collector on a phone still needs the model
registered from a path the backend can read (the NFS mount today); that
is documented in the collector guide.

## Risks / Trade-offs

- [Two schemas for one entity drift apart] → a contract test compares
  `CreateRunRequest` and `RunResponse` property sets and enumerations.
- [Validator and CHECK disagree] → a test posts every CHECK-violating
  shape and asserts `invalid_request` with the right field, never `500`.
- [Slow SSE subscriber loses events] → documented as expected; the client
  re-fetches on reconnect, and REST is authoritative.
- [Dev proxy buffers the stream] → an integration test opens the stream
  through `dev.ts` and expects the first keep-alive within the interval.
- [Burst of posts triggers heavy `results` recomputation] → debounced
  invalidation; the fold is already cheap for tens of thousands of runs.
- [Client forgets to upload artifacts first] → the error names the field;
  the collector guide shows the order (artifacts, model lookup, run).

## Migration Plan

Additive only: no migration, no data change. Deploy the backend, regenerate
`openapi/openapi.json` and the dashboard types, rebuild the dashboard.
Rollback is redeploying the previous binary; rows created through the API
are ordinary runs.

## Open Questions

- Whether `run.created` should also carry the results-row key so the
  dashboard can invalidate only that configuration. Not needed for the
  current data volume; the payload can grow additively later.
