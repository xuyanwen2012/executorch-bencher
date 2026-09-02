# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Backend (Rust 2024 edition, Axum 0.8 + SQLx 0.9 on embedded SQLite) plus a dashboard
(Bun 1.4 + React 19 + Tailwind, no Node.js) for collecting and viewing Android LLM
benchmark runs. Single crate: `executorch-bencher` (lib + default bin) and a second
bin `gen-openapi`. The dashboard is a separate Bun project in `dashboard/`.

This repo is **spec-driven** (OpenSpec). `openspec/specs/*/spec.md` are the
authoritative requirements; code comments cite them as
`See \`specs/<capability>\` - "<Requirement title>"`. Keep that convention when
adding behaviour, and use the `opsx:*` / `openspec-*` skills for new changes
(archived change history is in `openspec/changes/archive/`).

## Commands

Two database profiles: `.env` = **dev/mock** (`data/dev/`), `.env.real` = **real**
(`data/real/`, phones + Linux boxes). Neither is auto-loaded by cargo; `just`
recipes take the profile as first argument and default to `dev`. Never put fake
data in the real profile; the mock seeder refuses anything but `data/dev/`.

```sh
set -a; source .env; set +a          # or .env.real; DATABASE_URL is required
cargo run                            # backend, default LISTEN_ADDR 0.0.0.0:3000
just serve-backend [real]            # 127.0.0.1:3100 (port 3000 is taken on this machine)
cargo test                           # whole suite; hermetic, needs no .env, no Bun, no live DB
cargo test --test runs_api           # one integration test file (tests/runs_api.rs)
cargo test --test runs_api list_runs # one test by name substring
cargo run --bin gen-openapi          # regenerate openapi/openapi.json (no DB needed)
just seed-mock                       # fake Android + Linux runs into the dev DB (idempotent)
just import-all                      # every imports/*/manifests/*.json into the real DB (idempotent)
just import-log <manifest.json>      # one manifest
just integrity [real]                # read-only storage/DB reconciliation report
cargo run --example e2e_insert_retrieve   # older single-run smoke example (needs an env loaded)
```

Dashboard (from `dashboard/`):

```sh
bun install
bun run dev            # http://localhost:3001, proxies /api and /health to BACKEND_URL (default 127.0.0.1:3000)
just serve-dashboard   # from repo root: port 3101, proxying to the just backend on 3100
bun run build          # writes dist/. NEVER bare `bun build` - it skips the Tailwind plugin
bun run check          # tsc --noEmit + generated-API-type drift check
bun test               # unit tests (happy-dom preloaded via bunfig.toml)
bun test src/lib/format.test.ts   # single test file
bun run generate-api   # regenerate src/api/schema.d.ts from ../openapi/openapi.json
```

### After any route or schema change (mandatory)

The OpenAPI contract is a two-link chain and both links have drift tests:

```sh
cargo run --bin gen-openapi              # Rust types/routes -> openapi/openapi.json
(cd dashboard && bun run generate-api)   # openapi.json -> dashboard/src/api/schema.d.ts
```

`tests/openapi_drift.rs` fails `cargo test` and `scripts/check-api-drift.ts` fails
`bun run check` when their side is stale. Commit both generated files with the change.
`openapi/openapi.json` and `schema.d.ts` are generated; never hand-edit them.

## Backend architecture

**Layering.** Each HTTP concern is a pair: `<thing>_api.rs` (Axum handlers, utoipa
annotations, exposes `router() -> OpenApiRouter<AppState>`) over a storage/domain
module with no HTTP knowledge:

| HTTP module | Backing module | Routes |
|---|---|---|
| `runs_api.rs` | `runs.rs` (`NewRun`, `insert_run`, `get_run`, `list_runs`, cursor paging) | `GET /api/v1/runs`, `GET /api/v1/runs/{id}` |
| `results_api.rs` | `results.rs` (group-by-configuration median/min/max/n, computed in Rust, not SQL) | `GET /api/v1/results` |
| `artifacts_api.rs` | `artifact_store.rs` (content-addressed `sha256/<prefix>/<sha256>`, temp-then-rename, zstd for some kinds) | `POST/GET /api/v1/artifacts...` |
| `models_api.rs` | `model_registry.rs` (`ModelStorage` trait; only `ExternalModelStorage` exists, registers `.pte` in place) | `/api/v1/models...` |
| `runs_write_api.rs` | `runs.rs` + `domain::validate_*` | `POST /api/v1/runs` |
| `events_api.rs` | `events.rs` (`EventBus`, broadcast) | `GET /api/v1/events` (SSE) |
| `version_api.rs` | hand-maintained `API_VERSION` / `SCHEMA_VERSION` consts | `GET /api/v1/version` |

`http.rs` builds the whole route table once (`build_router`) and is shared by the
server, the `gen-openapi` bin, and the drift test, so the OpenAPI document and the
live routes cannot diverge. It also mounts Swagger UI at `/docs`, the raw doc at
`/openapi.json`, and (when `DASHBOARD_DIST` is set) the built dashboard as a
SPA fallback that never shadows registered routes.

**Shared pieces.** `domain.rs` holds validated value types (`Sha256Hex`, `ExitStatus`,
`CorrectnessResult`) that enforce invariants at the application boundary.
`api_error.rs` is the single JSON error envelope (`{ "error": { code, message, ... } }`);
match on `code`, not `message`. `config.rs` parses every env var (data roots,
`LISTEN_ADDR`, `DASHBOARD_DIST`, `MAX_ARTIFACT_UPLOAD_BYTES`, etc.) and creates the
storage roots at startup. `integrity.rs` is a read-only report and is deliberately
never run at startup.

**Platforms and device classes.** Every run has `platform` (`android` | `linux`),
`device_class` (`internal` lab device | `external` retail phone or Linux box), and
`device_serial` (serial or hostname). Rust models the snapshot as
`runs::HostState::{Android(AndroidDeviceState), Linux(LinuxHostState)}`.
`AndroidDeviceState` is all-optional description (build, kernel, SoC, GPU, driver,
memory, uptime, battery, temperatures, throttling) plus `Option<AndroidLabConfig>`
(BSP, SUMD driver, pinned GPU/MIF/INT clocks). Internal devices must have the full
snapshot (`insert_run` refuses otherwise); external ones record what they can. The
`host_*` columns hold the description on both platforms. A DB CHECK enforces the
per-(platform, class) shape. The results key adds `platform`, `device_class`, and
`host_accelerator`. `executable_sha256` is nullable: null means "not preserved",
never a placeholder. All real data so far is external.

**Data rules worth knowing.** Runs are immutable rows; corrections are additive.
Retries of the same repetition number are separate rows (no uniqueness constraint).
Write paths: `POST /api/v1/runs` (`runs_write_api.rs`; one complete run per call,
request mirrors `RunResponse`, enums accepted as strings and validated by field,
client-assigned id, `409 conflict` on repeat), the `import-observer-log` bin (a
`llama_main` `PyTorchObserver` log plus a JSON manifest, idempotent on `(log sha256,
tag, rep)`; raw logs and manifests in `imports/`), and `runs::insert_run` from Rust.
There is no in-progress run state and no finalize/progress endpoint by design.
`events.rs` is an in-process broadcast bus; the run, artifact, and model handlers
publish after their write and `GET /api/v1/events` streams it as SSE (signal only, no
replay). Artifact writes go to `temporary/` first and are renamed into place so the DB
never references a missing file. Full storage design: `docs/storage.md`; collector
guide: `docs/collector.md`, reference script `examples/post_run.py`.

**Database.** `db::connect_and_migrate` opens SQLite with WAL, `foreign_keys=ON`,
5s busy timeout, and runs `sqlx::migrate!()` from `migrations/` on every start.
Migrations are timestamped `.up.sql`/`.down.sql` pairs. SQLite cannot alter column
nullability, so column-shape changes rebuild `runs` (create new, `INSERT ... SELECT`,
drop, rename, re-index); see the two existing rebuild migrations for the pattern. The code uses runtime
`sqlx::query`/`query_as` (no `query!` macros), so no live database or `.sqlx/`
cache is needed to compile; the README's offline-cache section only matters if
macros are introduced.

**Tests.** Integration tests live in `tests/` and use `tests/common/mod.rs`:
`test_context()` gives a migrated temp SQLite DB plus temp storage roots,
`ctx.config()` a matching `Config` for HTTP tests (via `tower::ServiceExt::oneshot`),
`seed_new_run`/`build_new_run` a fully populated `NewRun`. Prefer these over
touching the real `data/` directory.

## Dashboard architecture

Bun-native: `dev.ts` uses `Bun.serve` routes with an HTML entrypoint import and
proxies `/api/*` and `/health` to the backend so the browser is same-origin;
`build.ts` runs `Bun.build` with `bun-plugin-tailwind`. There is no Vite/webpack.

`src/api/client.ts` wraps `openapi-fetch` with the generated `paths` type
(`baseUrl: ""`, same-origin in both dev and production). Data fetching is
TanStack Query with `retry: false` by design. Routing is `react-router` with three
pages under `Layout`: `ResultsPage` (`/`), `RunsPage` (`/runs`), `RunDetailPage`
(`/runs/:id`). Pure helpers in `src/lib/` (`collapse`, `filters`, `format`) carry
the unit tests.
