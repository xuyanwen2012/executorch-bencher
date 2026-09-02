## 1. Dependencies

- [x] 1.1 Add `utoipa`, `utoipa-axum`, and `utoipa-swagger-ui` (latest versions compatible with Axum 0.8.9) to `Cargo.toml`, enabling the `chrono`/`uuid` feature flags utoipa needs to derive schemas for existing timestamp/UUID fields, and verify `cargo build` succeeds.

## 2. Shared error envelope

- [x] 2.1 Add an `ApiError` type (`{"error": {"code","message","details","request_id"}}`) with `Serialize` + `ToSchema`, and an `IntoResponse` impl that maps it to the appropriate status code, in a new shared module (e.g. `src/api_error.rs`).
- [x] 2.2 Replace the ad hoc error responses in `runs_api.rs`, `artifacts_api.rs`, and `models_api.rs` (including the existing 400/404/410/413/500/501 cases) with `ApiError`, choosing a stable `code` per failure category, and verify no handler returns a bare string or a differently-shaped JSON error body.
- [x] 2.3 Update existing integration tests that assert on the old ad hoc error shapes to assert on the new `ApiError` envelope instead, and verify `cargo test` passes. (No existing test asserted on error body shape, only status codes, which are unchanged — full suite passes as-is.)

## 3. OpenAPI schema annotations

- [x] 3.1 Add `ToSchema` (and `IntoResponses`/`ToResponse` where useful) to the existing API-layer DTOs (`RunResponse`, `ArtifactMetadataResponse`, `UploadResponse`, `ArtifactView`, `ModelAssetResponse`, `ModelAssetSummary`, `ApiError`) without deriving it on domain or SQLx row types, and verify `cargo build` succeeds.
- [x] 3.2 Add schema-level documentation (descriptions, examples, nullability) for the fields actually returned today: SHA-256 fields (artifact/model) constrained to lowercase 64-hex, and nullable response fields (`output_preview`, `finished_at`, optional artifact/model references), and verify the relevant fields appear correctly in the generated document (task 6.1). Device/performance/build-identity `Run` fields (clocks, temperatures, uptime, speeds, token counts, git/executable identity) are not returned by `RunResponse` and are out of scope per proposal.md/design.md — do not add them to `RunResponse` as part of this task.
- [x] 3.3 Ensure `ExitStatus`, `CorrectnessResult`, `ArtifactKind`, and `ModelStorageMode` derive `ToSchema` with their exact stable snake_case values (not free-form strings), and verify the generated document's enum schemas list exactly the values in ingestion-service/spec.md and api-documentation/spec.md.

## 4. Route wiring and operation documentation

- [x] 4.1 Convert route registration in `http.rs`, `runs_api.rs`, `artifacts_api.rs`, and `models_api.rs` to `utoipa-axum`'s `OpenApiRouter`, attaching `#[utoipa::path(...)]` (or the router-level equivalent) to every existing route: `GET /health`, `GET /api/v1/runs/{run_id}`, the four artifact operations, and the four model operations, and verify each still responds correctly via existing tests.
- [x] 4.2 Document the artifact upload operation accurately as query params (`kind`, `original_name`) plus a raw streamed request body (not multipart), including the max upload size, dedup behavior, and returned metadata, and verify this matches `artifacts_api.rs`'s real implementation.
- [x] 4.3 Document artifact content/download responses as binary/streaming with the correct media type, and document the distinct "database record exists but file unavailable" error case using the new `ApiError` envelope.
- [x] 4.4 Assign operations to the `system`, `runs`, `models`, and `artifacts` tags per api-documentation/spec.md (no `events` or `analysis` tag), and assign each operation a stable `operationId` (e.g. `getRun`, `uploadArtifact`, `getArtifactMetadata`, `downloadArtifact`, `getArtifactContent`, `registerModel`, `listModels`, `getModel`, `verifyModel`, `healthCheck`).
- [x] 4.5 Configure top-level OpenAPI metadata (title, factual non-marketing description, API version, no `license`, no `servers` block) via the `OpenApi` derive/builder.

## 5. Documentation endpoints

- [x] 5.1 Add `GET /openapi.json` serving the generated `utoipa` document as JSON, and verify it returns a well-formed OpenAPI 3.x document.
- [x] 5.2 Add `GET /docs` serving `utoipa-swagger-ui` backed by `/openapi.json`, unversioned (not under `/api/v1`), and verify it loads and references `/openapi.json`.
- [x] 5.3 Add `GET /api/v1/version` returning `api_version`, `schema_version`, and `minimum_runner_version` as maintained constants and `server_version` derived from `env!("CARGO_PKG_VERSION")`, and verify the response matches `Cargo.toml`'s package version.

## 6. Generated contract artifact

- [x] 6.1 Add a `[[bin]]` target (e.g. `gen-openapi`) that builds the same `OpenApi` document and writes it to `openapi/openapi.json` with deterministic key ordering, and verify `cargo run --bin gen-openapi` produces the file.
- [x] 6.2 Add a test asserting the checked-in `openapi/openapi.json` matches the document the running server currently serves at `/openapi.json` (drift check), and verify it fails when the two are made to differ and passes otherwise.
- [x] 6.3 Run the regeneration command once to produce the initial checked-in `openapi/openapi.json` and commit it. (File generated and present on disk; actual `git commit` deferred to the user's explicit request per repo convention.)

## 7. Tests

- [x] 7.1 Add a test that `/openapi.json` returns valid, parseable OpenAPI 3.x JSON with the expected title, description, version, and tag list (`system`, `runs`, `models`, `artifacts` only).
- [x] 7.2 Add tests asserting the `ExitStatus`, `CorrectnessResult`, `ArtifactKind`, and `ModelStorageMode` schemas enumerate exactly their documented stable values.
- [x] 7.3 Add a test asserting SHA-256 fields (artifact/model) are documented and constrained to lowercase 64-hex.
- [x] 7.4 Add a test asserting nullable fields that actually exist in a response (`output_preview`, `finished_at`, optional artifact/model references) remain nullable/optional in the schema.
- [x] 7.5 Add a test asserting the run response schema in the generated document lists only the fields `RunResponse` actually serializes, with no clock/temperature/uptime/speed/token-count/build-identity field present.
- [x] 7.6 Add a test asserting the artifact upload operation is documented with `kind`/`original_name` query parameters and a raw request body (not a multipart form).
- [x] 7.7 Add a test asserting artifact content/download operations declare binary/streaming response content, not a JSON string body.
- [x] 7.8 Add a test asserting `/api/v1/version` matches its documented schema and reflects the built package version.
- [x] 7.9 Add a test asserting all `operationId` values are present and unique across the document.
- [x] 7.10 Add a test asserting the document contains no path, tag, or orphan schema for run creation, finalize, progress, list, or `/api/v1/events`.
- [x] 7.11 Add a test asserting Swagger UI at `/docs` responds successfully and references `/openapi.json`.
- [x] 7.12 Run the full existing test suite (including the updated error-envelope assertions from task 2.3) and verify it passes.

## 8. Documentation

- [x] 8.1 Add `docs/api.md` covering: where to access Swagger UI and the OpenAPI JSON, how to regenerate the checked-in contract (`cargo run --bin gen-openapi`), how `api_version`/`schema_version`/`minimum_runner_version`/`server_version` work and are checked for compatibility, which endpoints are currently implemented versus documented as gaps (run lifecycle, events), and how SSE will differ from the documented REST endpoints once it exists.
- [x] 8.2 Add Python and TypeScript client generation examples (`openapi-python-client generate --path openapi/openapi.json`; a fetch-based TS client) to `docs/api.md`, noting no client packages are generated or committed in this change, and noting generated clients should be wrapped in small ergonomic application-specific APIs rather than edited manually.
- [x] 8.3 Update `README.md` with a pointer to `docs/api.md` and the Swagger UI / OpenAPI JSON locations.

## 9. Verification

- [x] 9.1 Run `cargo fmt` and verify no diff remains.
- [x] 9.2 Run `cargo test` and verify the full suite passes, including the new OpenAPI-validity and error-envelope tests.
- [x] 9.3 Run `cargo clippy --all-targets -- -D warnings` and verify it is clean.
- [x] 9.4 Start the backend locally, open `/docs` in a browser, and confirm Swagger UI loads and lists all documented operations under the expected tags. (Port 3000 was occupied by an unrelated host process; verified instead via a temporary local bind reverted immediately after — `/docs/` returns 200 and serves `swagger-ui.css`, `/docs` redirects to `/docs/`.)
- [x] 9.5 Exercise `GET /api/v1/runs/{run_id}`, artifact upload, model registration, and model verification through the Swagger UI "Try it out" feature and confirm each responds as documented, including an error case exercising the new `ApiError` envelope. (Exercised via direct HTTP requests against the same running server rather than the Swagger UI's JS "Try it out" button specifically: upload → 201, metadata → 200, invalid kind → 400 `ApiError`, unknown artifact/run → 404 `ApiError`, register with bad path → 400 `ApiError`, register + verify a real model → 201/200.)
- [x] 9.6 Confirm `openapi/openapi.json` matches the runtime `/openapi.json` output (task 6.2's test passing is sufficient evidence; also diffed live `/openapi.json` against the checked-in file directly — identical).
- [x] 9.7 Produce a final report listing: modified files, tests run, documented routes, the explicit gap list (run create/finalize/progress/list, SSE events, authentication, and `RunResponse`'s unexposed device/performance/build-identity fields), and any remaining follow-ups (e.g. CI wiring once a workflow exists).
