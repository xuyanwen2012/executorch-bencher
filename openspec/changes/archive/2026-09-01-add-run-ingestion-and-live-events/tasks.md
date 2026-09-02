## 1. Run creation over HTTP

- [x] 1.1 Add `conflict` to `api_error.rs` (`409`) and update the code list in the envelope docs; verify with a unit test that the envelope serialises `code: "conflict"` at status 409.
- [x] 1.2 Add `CreateRunRequest` (flat, mirrors `RunResponse` field names, units, enums, plus artifact IDs) in a new `src/runs_write_api.rs`, with a conversion into `NewRun` that parses enums, canonicalises the JSON columns via `domain::validate_*`, builds `HostState` per platform/device class (Android lab config all-or-none; Linux rejects lab fields), truncates `output_preview` to the configured length, and returns field-named errors; verify with unit tests covering each rejection rule and one Android-internal, one Android-external, and one Linux happy path.
- [x] 1.3 Add existence checks for `model_asset_id` and every artifact ID before insert, mapping a missing one to `invalid_request` with `details.field`; verify with an integration test posting a run with a missing `stdout_artifact_id` and asserting `400`, the field name, and zero rows.
- [x] 1.4 Implement `POST /api/v1/runs` returning `201` with `RunResponse`, mapping a primary-key violation to `409 conflict`; register it in `http::build_router`; verify with integration tests for a succeeded run (then visible via `GET /runs/{id}`, `GET /runs`, and `GET /results`), a crashed run counted as not succeeded, an external phone with null lab fields, and a duplicate ID.
- [x] 1.5 Add a test that submits every shape the platform/device-class CHECK rejects and asserts each is refused by validation with `invalid_request` (never `500`).
- [x] 1.6 Add the `sha256` query filter to `GET /api/v1/models` (well-formed hash → exact match or empty list; malformed → `400`); verify with integration tests for the three cases.

## 2. Live events

- [x] 2.1 Add an `Event` enum (`RunCreated { summary }`, `ArtifactCreated { id, kind, sha256, size_bytes }`, `ModelRegistered { id, original_name, sha256 }`) with `ToSchema` payload types, and a `broadcast::Sender<Event>` in `AppState` created by `http::router`; verify the library builds and `AppState` construction in tests passes a sender.
- [x] 2.2 Publish from the run creation, artifact upload, and model registration handlers after their writes succeed; verify with a test that subscribes to the sender, performs each write through the router, and receives the matching event with the new record's ID.
- [x] 2.3 Implement `GET /api/v1/events` with axum `Sse`, a process-scoped monotonically increasing event `id`, JSON `data`, `KeepAlive` every 15 s, and stream termination on `Lagged`; verify with an integration test that opens the stream, posts a run, and reads a `run.created` event containing its ID, plus a test that a keep-alive comment arrives when idle (use a short interval in tests).
- [x] 2.4 Add `docs/api.md` wording for the stream (signal, not state; no replay) and remove SSE and run creation from the "documented as a gap" list, leaving authentication.

## 3. Contract and version

- [x] 3.1 Document `POST /api/v1/runs` (request schema, `201`/`400`/`409`), the `sha256` model filter, and `GET /api/v1/events` (`text/event-stream`, description naming the events, referenced payload components) with utoipa; add the `events` tag; bump `API_VERSION` to `1.3`; regenerate `openapi/openapi.json`; verify `cargo test` passes including `tests/openapi_drift.rs`.
- [x] 3.2 Update `tests/openapi_contract.rs`: presence of the new operations and tag, absence of finalize/progress, request/response run schema agreement (same property names and enumerations for shared fields), event payload components present; verify the tests pass.

## 4. Dashboard

- [x] 4.1 Regenerate `dashboard/src/api/schema.d.ts`; verify `bun run check` passes.
- [x] 4.2 Add `src/lib/live.ts` with a `useLiveEvents` hook (EventSource on `/api/v1/events`, connection state, `run.created` → debounced `invalidateQueries` for `results` and `runs`) and a small pure `coalesce` helper; verify unit tests for the coalescing (many events in a window → one callback) and for parsing event payloads.
- [x] 4.3 Show a live/off indicator in the layout and wire the hook on the results and runs pages, preserving filters and paging on refetch; verify by running the dev server against a backend, posting a run with `examples/post_run.py`, and observing the row update without reload (record the observation in the PR notes) and `bun test` passing.
- [x] 4.4 Confirm the Bun dev proxy streams `/api/v1/events` unbuffered; verify with a script or test that reads the first keep-alive through port 3001 within the keep-alive interval.

## 5. Collector guide and example

- [x] 5.1 Write `docs/collector.md`: order of operations (upload artifacts → resolve model asset by `sha256` or register by backend-readable path → `POST /api/v1/runs`), the request fields per platform and device class with what an external phone may leave null, retry semantics around `409`, and the events stream; verify the document's example request validates against the checked-in OpenAPI schema (a test or a one-off script run noted in the task).
- [x] 5.2 Add `examples/post_run.py` using only the Python standard library: uploads a stdout artifact, looks up the model by hash, posts a run built from a `PyTorchObserver` line and host facts, prints the run URL; verify by running it against the dev profile backend and seeing the run on the dashboard.
- [x] 5.3 Update `README.md` and `CLAUDE.md` (write path exists; events; collector guide pointer); verify the text matches the implemented routes listed by `GET /openapi.json`.
