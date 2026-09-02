## Context

See proposal.md for motivation. Key constraints established during
investigation of the current codebase (Axum 0.8.9, single crate, no
`utoipa`/auth/CI today):

- Only `GET /api/v1/runs/{run_id}` exists for runs; create/finalize/progress/
  list and the SSE events endpoint have no HTTP wiring at all.
- Artifact upload takes `kind`/`original_name` as query params and streams
  the raw request body — it is not multipart, contrary to a generic
  assumption about this kind of API.
- Error responses today are ad hoc per handler (bare strings in some places,
  JSON objects in others); real status codes in use include 400, 404, 410
  (Gone, for a DB record whose file is missing), 413, 500, and 501 (Not
  Implemented, for managed-mode model verification).
  status codes 401/403/409/422/503 are not used anywhere today.
- No authentication exists anywhere in the service.
- `gpu_clock_mhz`/`mif_clock_mhz`/`int_clock_mhz` values of 980/5333/934 are
  hardcoded only in a test fixture (`runs.rs`); there is no server-side
  default logic for these columns.
- `Cargo.toml` has no `license` field, and no deployment/reverse-proxy
  config exists to derive an OpenAPI `servers` entry from.
- No `python/`, `dashboard/`, `frontend/`, or `runner/` package exists in
  this repo.

## Goals / Non-Goals

**Goals:**
- Generate the OpenAPI document from real Axum routes and Rust types via
  `utoipa`, serve it at `/openapi.json`, serve Swagger UI at `/docs`, and
  serve `/api/v1/version`.
- Document every implemented route accurately, including where it diverges
  from generic API-documentation assumptions (artifact upload shape, extra
  status codes).
- Unify error responses onto one `ApiError` envelope, since the document
  cannot describe one consistent error schema while handlers return several
  different shapes.
- Keep a checked-in `openapi/openapi.json` in sync with the runtime document
  via a regeneration command and a drift-check test.

**Non-Goals:**
- Implementing run creation, finalize, progress, list, or SSE events — none
  exist today, and building them is new benchmark functionality.
- Expanding `RunResponse` to expose device/performance/build-identity
  fields it does not currently return (see Decisions).
- Adding authentication, or documenting a partial/invented security scheme.
- Adding a CI workflow (none exists in this repo yet).
- Changing the SQLite schema or artifact-storage architecture.
- Generating or committing Python/TypeScript client packages.
- Actually invoking `openapi-python-client` (or any external codegen tool)
  as part of this change's verification.

## Decisions

**Utoipa integration style**: use `utoipa-axum`'s `OpenApiRouter` to declare
routes so operations, path parameters, and request/response schemas are
attached at the same call site as the route registration, instead of a
separate hand-maintained `#[utoipa::path(...)]` block per handler drifting
from the route table. Alternative considered: raw `utoipa::OpenApi` derive
with routes declared independently — rejected because it duplicates the
route table and is the exact kind of drift the task is meant to eliminate.

**DTO boundary**: continue the existing pattern of dedicated API response
structs (`RunResponse`, `ModelAssetResponse`, etc.) distinct from domain
structs (`Run`, `ModelAsset`) and SQLx rows — add `ToSchema` to the API
layer structs only. Alternative considered: deriving `ToSchema` on domain
structs directly — rejected because it would leak internal representation
details (and any future domain-only fields) into the public contract.

**Clock "defaults"**: moot as of the `RunResponse` field-scope decision
above — `gpu_clock_mhz`/`mif_clock_mhz`/`int_clock_mhz` are not exposed by
any endpoint, so there is no field to attach an example/default value to.
Recorded here for history: the task's suggested clock defaults
(980/5333/934) are not real server defaults (no default is applied
anywhere; the columns are plain non-nullable `i64` on `Run`, unexposed via
HTTP) — if these fields are exposed by a future change, they should be
documented as illustrative example values, not a schema-level `default`.

**`RunResponse` field scope (discovered during implementation)**: reading
the actual `runs_api.rs` during implementation showed `RunResponse` (the
only run-read response, from `GET /api/v1/runs/{run_id}`) exposes only
`id`, `started_at`, `finished_at`, `exit_status`, `correctness_result`,
`output_preview`, and artifact/model summaries — none of the device state,
clocks, temperatures, uptime, prefill/decode speed, token counts, or build
identity fields the `Run` domain struct carries. The original task assumed
these fields were part of a documented run response; they are not exposed
by any endpoint today. Decision: do not expand `RunResponse` to add them.
Unit/nullability documentation is scoped to the fields the API actually
returns (enums via the separate enum requirement; `output_preview` and the
artifact/model summary fields for nullability); the undocumented
device/performance field set is reported as a contract gap alongside
run-lifecycle/events, not silently added as a side effect of a
documentation-only task. Alternative considered: expand `RunResponse` now
since the data already exists in the domain layer — rejected because it is
new data exposure, not "describing the existing API accurately," and
mirrors the same scope-creep risk already ruled out for run-lifecycle
endpoints.

**Error taxonomy**: `ApiError.error.code` is an open, documented string
field (not a closed enum) with a canonical list of currently-used values
given as examples in its schema description (e.g. `invalid_request`,
`not_found`, `artifact_file_missing`, `payload_too_large`,
`not_implemented`, `internal_error`). A closed enum would need to be
extended in lockstep with every future handler; an open, documented string
matches the task's own error-format example (`"code": "invalid_request"`)
without over-constraining it.

**Status codes beyond the task's suggested list**: the existing 410 (Gone,
missing artifact file) and 501 (Not Implemented, managed-mode verify)
responses are documented as-is alongside the task's suggested set — the
task's own Artifact API section explicitly anticipates the "database record
exists but local file is unavailable" case, and hiding a real response code
would make the contract less accurate, not more.

**Spec-generation command shape**: a dedicated `[[bin]]` target (e.g.
`gen-openapi`) writes `openapi/openapi.json`, run via
`cargo run --bin gen-openapi`. A separate `#[test]` only *asserts* the
checked-in file matches the runtime document (drift check) — it does not
regenerate it. Alternative considered: a `#[test]` that writes the file as
a side effect — rejected because it would fire on every `cargo test` run
and silently "fix" drift instead of catching it.

**Verification of client-generation readiness**: validate that
`/openapi.json` is a structurally valid OpenAPI 3.x document (e.g. via a
Rust OpenAPI-parsing crate or `serde_json` + light structural assertions on
required top-level fields), and document `openapi-python-client`/TS
generation commands for developers to run themselves. Not invoking those
external tools during this change's own verification avoids pulling a
Python toolchain into a pure-Rust repo for a one-time smoke test.

**Scope for run-lifecycle and events**: excluded entirely from the
generated document (no operations, no orphan schemas, no `events`/
`analysis` tags), per proposal.md. This keeps the contract honest about
what is actually callable today.

**`/health` and `external_path`**: `/health` is documented under the
`system` tag despite not being under `/api/v1`, since it is a real, working
endpoint. `ModelAssetResponse.external_path` is documented as currently
exposed and unauthenticated (no security model exists to gate it on), with
a note in its schema description — adding protection would be new auth
functionality, out of scope here.

**License / servers metadata**: omitted from the OpenAPI document — no
`license` field exists in `Cargo.toml` and no deployment/base-path
configuration exists to derive a `servers` entry from correctly.

## Risks / Trade-offs

- **[Breaking error-response change]** → Existing integration tests
  asserting on today's ad hoc error bodies will need updating in the same
  change; there are no external consumers yet, so the blast radius is
  contained to this repo's own test suite.
- **[Contract gap may read as incomplete]** → The final report explicitly
  lists run-lifecycle/events as an unimplemented gap rather than silently
  omitting them, so downstream readers (including future implementers of
  those endpoints) know the gap is deliberate, not an oversight.
- **[Utoipa/Axum 0.8 compatibility]** → `utoipa-axum` versions track Axum
  major versions; pin to the latest `utoipa`/`utoipa-axum`/`utoipa-swagger-ui`
  releases compatible with Axum 0.8 at implementation time and record the
  exact versions in `Cargo.toml`.

## Migration Plan

No data migration. Rollout is additive except for the error-response shape
change: deploy the new binary; existing clients of the ad hoc error bodies
(none exist outside this repo's own tests) would need to adapt to the new
`ApiError` envelope. Rollback is a straightforward binary revert since no
schema or storage-layer changes are involved.
