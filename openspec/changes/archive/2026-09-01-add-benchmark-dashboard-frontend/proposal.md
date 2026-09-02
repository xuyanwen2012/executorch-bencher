## Why

The backend can store benchmark runs, artifacts, and model assets, but the
only way to look at a result today is `GET /api/v1/runs/{id}` (which omits
every performance and device field) or the SQLite file itself; there is no
way to list runs at all. The question the project exists to answer - "what
prefill tok/s does this model/quantization get on this device at this
commit, with this driver and BSP" - has no view. This change sets up both
halves: a read-only browser dashboard whose headline is a per-configuration
results table you can eyeball, and the backend read surface it depends on.
A Python runner that writes runs over HTTP comes in a later change; this
one is presentation only.

## What Changes

- Add a `dashboard/` package in this repo: a Bun + React + TypeScript +
  Tailwind CSS single-page app built, served in development, and tested by
  Bun's own toolchain (no Node/Vite), with a typed API client generated from
  the checked-in `openapi/openapi.json`. Light theme only.
- The dashboard has three pages. **Results** (`/`): one row per benchmark
  configuration (device, model, commit + dirty flag, SUMD driver, BSP,
  GPU/MIF/INT clocks, prompt), newest commit first, showing median /
  min–max / `n` prefill and decode tok/s over succeeded runs, failure and
  throttling counts, with filters on every dimension, columns that are
  constant across visible rows collapsed into a "shared configuration"
  line, and each row linking to its runs. **Runs** (`/runs`): a
  cursor-paginated newest-first list with exact-match filters, for
  diagnosis. **Run detail** (`/runs/:id`): every recorded field grouped,
  the referenced model, and each attached artifact with view/download.
- Add a small additive migration giving `runs` nullable `git_branch`,
  `git_commit_timestamp`, and `git_commit_subject` columns, so results can
  be ordered and labelled by commit history once the runner fills them.
- Add `GET /api/v1/results`: server-side grouping of succeeded runs by
  configuration with median/min/max/`n` statistics, failure/throttle
  counts, commit metadata, and `facets` (distinct filter values), capped
  at 500 groups.
- Add `GET /api/v1/runs`: a paginated (opaque keyset cursor), filterable,
  newest-first run listing returning per-run summaries.
- Expand `GET /api/v1/runs/{id}`'s response to return every recorded field
  (device state, clocks, temperatures, build identity, token counts,
  throughput, git metadata). Additive only; existing fields keep their
  names and shapes.
- Optionally serve the built dashboard: when `DASHBOARD_DIST` is set, Axum
  serves that directory at `/` with a single-page-app fallback, so one
  process serves API and UI. Unset means unchanged behavior. In
  development the Bun dev server proxies API requests to the backend, so
  no CORS is introduced.
- Bump the documented `api_version` from `1.0` to `1.1`; `schema_version`
  stays at 1 (the migration is backward compatible). Regenerate
  `openapi/openapi.json`.
- Explicitly out of scope: any HTTP write path for runs, charts or
  trend/comparison views, per-model metadata beyond the `.pte` filename,
  authentication, CORS, live updates (SSE), browser end-to-end tests, CI
  wiring, dark theme.

## Capabilities

### New Capabilities
- `benchmark-dashboard`: the browser UI for viewing benchmark results -
  its toolchain contract (Bun-native build, typed client generated from the
  OpenAPI document, dev proxy), the results, runs, and run detail pages,
  artifact viewing/downloading, time presentation, and how it handles
  missing data and API errors.

### Modified Capabilities
- `benchmark-schema`: runs gain optional git commit metadata (branch,
  commit timestamp, commit subject).
- `ingestion-service`: gains a grouped results operation, a paginated and
  filterable run-listing operation, a single-run read exposing the full
  recorded run, and optional serving of the built dashboard's static
  assets with an SPA fallback.
- `api-documentation`: the "excludes unimplemented operations and
  unexposed fields" requirement is narrowed - results, run listing, and the
  expanded run fields become implemented and therefore documented, while
  run create/finalize/progress and the events endpoint remain excluded;
  numeric fields document their units.

## Impact

- **New code**: `dashboard/` (package.json, `bunfig.toml`, `tsconfig.json`,
  `dev.ts`, `build.ts`, `src/` React app, generated `src/api/schema.d.ts`,
  `bun test` unit tests); `src/results.rs` and `src/results_api.rs` in the
  backend.
- **Backend code**: one new migration; `src/runs.rs` (git metadata fields,
  list query with filters and cursor), `src/runs_api.rs` (list handler,
  expanded `RunResponse`), `src/http.rs` and `src/config.rs` (optional
  `DASHBOARD_DIST` static serving), `src/version_api.rs` (`api_version`
  bump), `examples/e2e_insert_retrieve.rs` (seeds git metadata),
  `openapi/openapi.json` regenerated.
- **Dependencies**: Rust gains `tower-http` (`fs` feature) and `base64`.
  The dashboard depends on `react`, `react-dom`, `react-router`,
  `@tanstack/react-query`, `openapi-fetch`, `bun-plugin-tailwind`
  (which bundles its own Tailwind v4 compiler), with `typescript` pinned
  to 5.x (`openapi-typescript` does not support TypeScript 7), `@types/*`,
  and `openapi-typescript` as dev dependencies. Bun 1.4.x is a new
  developer prerequisite and is installed on the current machine.
- **Tests**: existing OpenAPI contract tests that assert `GET /api/v1/runs`
  is absent and that the run response omits performance fields flip to the
  new contract; new integration tests cover results grouping and
  statistics, listing, filtering, pagination, the expanded run response,
  the migration, and static-asset serving.
- **Docs**: `README.md` and `docs/api.md` gain dashboard setup/run/build
  instructions and the updated "implemented vs. gap" list.
- **Consumers**: no external consumer exists yet; the dashboard is the
  first client of the generated contract.
