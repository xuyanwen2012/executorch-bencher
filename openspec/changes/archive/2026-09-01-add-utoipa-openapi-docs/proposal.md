## Why

The backend's HTTP API has no machine-readable contract today: request/response
shapes, enum values, units, and error semantics only exist as Rust source. The
Python benchmark runner and the future React/TypeScript dashboard both need a
generated, authoritative contract to build typed clients against, and human
developers need interactive documentation instead of reading handler source.
Investigation of the current codebase also surfaced two contract gaps worth
fixing while documenting it: error responses are shaped ad hoc per handler
(mixed bare strings and JSON objects), and several endpoints the eventual
contract will need (run creation/finalize/progress/list, live events) do not
exist yet at all.

## What Changes

- Add `utoipa` + `utoipa-axum` + `utoipa-swagger-ui` and annotate existing
  public request/response DTOs with `ToSchema`/`IntoResponses` so the OpenAPI
  document is generated from the real Axum routes and Rust types, not
  hand-maintained.
- Expose `GET /openapi.json` (full OpenAPI 3.x document), `GET /docs`
  (Swagger UI backed by it), and `GET /api/v1/version` (hardcoded
  `api_version`/`schema_version`/`minimum_runner_version` constants plus
  `server_version` derived from the Cargo package version).
- Document every currently-implemented route under `/api/v1/...` plus
  `/health`: `GET /api/v1/runs/{run_id}`, the artifact upload/metadata/
  content/download operations, and the model register/list/get/verify
  operations — matching their real request shapes (e.g. artifact upload is
  query params `kind`/`original_name` + a raw streamed body, not multipart)
  and real status codes (including the existing 410 Gone and 501 Not
  Implemented responses).
- **BREAKING**: Introduce one consistent `ApiError` JSON response shape
  (`{"error": {"code","message","details","request_id"}}`) across all
  handlers, replacing today's inconsistent per-handler error bodies (bare
  strings in some places, ad hoc JSON objects in others). Existing tests
  that assert on the old shapes are updated accordingly.
- Add a `[[bin]]` target that writes the generated document to the
  checked-in `openapi/openapi.json`, plus a test asserting that checked-in
  file matches the live `/openapi.json` output (drift check) — no CI
  workflow is added in this change since none exists yet.
- Add a test validating `/openapi.json` is a structurally valid OpenAPI 3.x
  document (parses correctly, has the expected metadata/tags/schemas/enum
  values), plus the other documentation-correctness tests listed in
  design.md. Client generation (Python/TypeScript) is documented with
  example commands but not executed or committed, since no client packages
  exist in this repo yet.
- Update `docs/` with where to find Swagger UI and the OpenAPI JSON, how to
  regenerate the checked-in contract, how versioning/compatibility works,
  and how SSE (once it exists) will differ from the documented REST
  endpoints.
- Explicitly out of scope: implementing `POST /api/v1/runs`,
  `POST /api/v1/runs/{run_id}/finalize`, `POST /api/v1/runs/{run_id}/progress`,
  `GET /api/v1/runs` (list), or `GET /api/v1/events` (SSE) — none of these
  exist today, and building them is new benchmark functionality, not API
  documentation. They are reported as an explicit contract gap rather than
  documented as operations or represented by orphan schemas. Also out of
  scope: authentication (none exists; not invented here), CI wiring, the
  `events`/`analysis` OpenAPI tags (nothing backs them yet), and SQLite
  schema/artifact-storage architecture changes.
- Also out of scope: expanding `RunResponse` (the only run-read response
  today) to expose the device/performance/build-identity fields the `Run`
  domain struct already carries — clocks, temperatures, uptime, prefill/
  decode speed, token counts, git commit/dirty, executable SHA-256, etc.
  `GET /api/v1/runs/{run_id}` does not return any of these today, so there
  is nothing real to attach unit/nullability documentation to; they are
  reported as a contract gap alongside run-lifecycle/events rather than
  added to the response as a side effect of this documentation task.

## Capabilities

### New Capabilities
- `api-documentation`: generated OpenAPI 3.x contract, `/openapi.json`,
  Swagger UI at `/docs`, `/api/v1/version`, and the checked-in
  `openapi/openapi.json` regeneration/drift-check workflow.

### Modified Capabilities
- `ingestion-service`: HTTP error responses across all existing endpoints
  change from inconsistent ad hoc shapes to one consistent `ApiError`
  response format.

## Impact

- **Code**: `src/http.rs` (route wiring, docs/version routes), `src/runs_api.rs`,
  `src/artifacts_api.rs`, `src/models_api.rs` (schema annotations, unified
  error responses), a new small module for the shared `ApiError` type and
  OpenAPI metadata/tags, a new `[[bin]]` target for spec generation.
- **Dependencies**: adds `utoipa`, `utoipa-axum`, `utoipa-swagger-ui` (and
  `utoipa`'s `axum_extras`/`chrono`/`uuid` feature flags as needed) to
  `Cargo.toml`.
- **Tests**: existing integration tests that assert on current ad hoc error
  bodies are updated; new tests are added for OpenAPI document validity,
  metadata/tag/schema presence, enum value correctness, and checked-in-file
  drift.
- **Docs**: `docs/api.md` (new) plus updates to `README.md` covering Swagger
  UI location, regeneration command, and versioning.
- **Consumers**: no existing Python runner or TypeScript dashboard client in
  this repo to break; this establishes the contract they will be generated
  against later.
