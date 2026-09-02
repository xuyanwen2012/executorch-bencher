## Context

See proposal.md for motivation. Constraints that shape the approach, from
the current codebase, the machine, and the design interview:

- Single workstation, single user, localhost. No auth, no CORS, no
  multi-user concerns. Teammates look at the screen.
- The backend is a single Rust crate (Axum 0.8, SQLx 0.9 on SQLite, utoipa
  5) with no frontend, no `tower-http`, and `/` currently 404s. Route
  registration goes through `utoipa-axum`'s `OpenApiRouter`, so every new
  route is documented at its registration site.
- Runs are only writable via the Rust API (`runs::insert_run`). A Python
  runner that writes over HTTP is the next change; this one is read-only.
  The e2e example is the only seeding path for verification.
- `Run` already carries every field the dashboard needs except git branch,
  commit timestamp, and commit subject, which the schema does not have.
  `command_args`, `input_parameters`, and `env_vars` are canonical JSON
  text. The prompt is fixed across all current experiments.
- The `runs` table has indexes on `started_at`, `device_serial`,
  `git_commit_sha`, `exit_status`, `correctness_result`,
  `sumd_driver_version`, `bsp_version`, and `model_asset_id`.
- Model identity is the `.pte` filename in `model_assets.original_name`
  (six today: three Llama variants times two quantizations); no separate
  family/size/quantization fields exist and the user chose to rely on the
  filename for now.
- Existing contract tests (`tests/openapi_contract.rs`) assert that
  `GET /api/v1/runs` is absent and that `RunResponse` omits the performance
  fields. Both encode the old contract and must flip.
- Bun 1.4.0 is installed. A spike verified: `bun init --react=tailwind`
  generates a `Bun.serve` app with HTML imports, HMR, and
  `bun-plugin-tailwind`; a streaming reverse proxy via `fetch` preserves
  `Content-Disposition`; `bun test` with happy-dom works. Gotchas: bare
  `bun build` ignores `bunfig.toml` plugins and emits unprocessed Tailwind,
  so production builds must go through a `build.ts` calling `Bun.build`
  with the plugin; the template pins TypeScript 7, which crashes
  `openapi-typescript`, so TypeScript must be pinned to 5.x; the Tailwind
  plugin bundles its own Tailwind v4 compiler.

## Goals / Non-Goals

**Goals:**
- A results page the user can eyeball: one row per configuration,
  newest commit first, grouping and statistics computed on the backend.
- Backend read endpoints sufficient for the three pages, with keyset
  pagination for the run list that stays correct under concurrent inserts.
- Keep the generated OpenAPI document the single contract; dashboard types
  derive from it with a drift check on both sides.
- Optional single-process deployment without changing default behavior.

**Non-Goals:**
- Pivot/matrix comparison views, charts, deltas between commits, or trend
  analysis. The results endpoint is designed so those can be built on it.
- Any write path from the UI, authentication, CORS, SSE, CI, Docker, dark
  theme, browser end-to-end tests, or bundling the dashboard into the Rust
  binary.
- Model family/size/quantization metadata. If filenames stop being
  self-describing, that belongs in the write-path change where the runner
  registers models.

## Decisions

**Page structure.** `/` Results (headline), `/runs` Runs (diagnostic
list), `/runs/:id` Run detail. Results rows link to `/runs?<full key>`.
Rationale: the interview established that the first thing to see is "what
does this configuration get", and individual runs are for diagnosis only.

**Configuration key.** (`device_serial`, `model_asset_id`,
`git_commit_sha`, `git_dirty`, `sumd_driver_version`, `bsp_version`,
`gpu_clock_mhz`, `mif_clock_mhz`, `int_clock_mhz`, `prompt_sha256`).
Driver, BSP, and clocks all change performance (user's answer to Q14), so
they are key dimensions, not warnings. `prompt_sha256` is in the key even
though the prompt is fixed today: it costs nothing now and prevents
silently pooling different prompt lengths later. Dirty runs are separate
rows marked `+dirty` rather than pooled with the clean commit. Alternative
considered: treating BSP/clocks as "consistency warnings" - rejected by the
user because they are real performance variables.

**Statistics: median, min, max, n over succeeded runs only.** No mean or
standard deviation (misleading at small n); no warmup exclusion (the
runner can choose not to record warmups). Decode statistics are computed
over succeeded runs that recorded a decode value, with their own `n`.
Non-succeeded runs, correctness failures among succeeded runs, and
throttled runs are counted per row as badges. Both prefill and decode
statistics are always returned; the "metric switch" in the UI only
chooses which one is emphasized, so the endpoint needs no `metric`
parameter. Alternative considered: a `metric` query parameter - rejected as
needless; the row is small either way.

**Results are computed server-side, in Rust, from one query.** The handler
runs a single `SELECT` of the key columns plus the metric/flag columns for
runs matching the filters (joined to `model_assets` for the name), folds
rows into a `BTreeMap<ConfigKey, Accumulator>` in Rust, computes medians by
sorting each accumulator's vector, orders rows, and truncates to 500 with a
`truncated` flag. SQLite has no median, and pushing grouping into SQL
would need `json_group_array` plus post-processing anyway; the Rust fold is
simpler to test and fast for tens of thousands of runs. Facets are five
cheap `SELECT DISTINCT` queries in the same handler, unfiltered by design
so the user can always widen a filter. Alternative considered: client-side
pivoting from the run list - rejected because the list is paginated and
the logic would live in TypeScript, untested by `cargo test`.

**Row ordering.** `COALESCE(git_commit_timestamp, earliest started_at)`
descending, then model name, then device serial. Recorded commit
timestamps win; runs without them (all runs until the runner fills the new
columns) fall back to first-run time, which approximates commit order for
a single developer's workflow.

**Git metadata migration.** One additive migration adds nullable
`git_branch TEXT`, `git_commit_timestamp TEXT`, `git_commit_subject TEXT`
to `runs` via `ALTER TABLE ... ADD COLUMN` (down: `DROP COLUMN`, SQLite ≥
3.35). No index; ordering happens in Rust after the fold. `NewRun`/`Run`
gain three `Option<String>`/`Option<DateTime<Utc>>` fields; the e2e
example sets them so the results page has labelled data.
`schema_version` stays 1 (backward compatible, additive). Alternative
considered: deferring to the write-path change - rejected because the
results page's row label and ordering axis would otherwise be designed
without the column it needs.

**Run listing: keyset pagination on `(started_at DESC, id DESC)` with an
opaque cursor.** The cursor is base64url of `"<started_at RFC3339>|<uuid>"`;
the next page adds `(started_at < ?) OR (started_at = ? AND id < ?)`.
Stable under concurrent inserts, uses the `started_at` index, opaque so the
encoding can change. Filters are exact-match `AND` clauses built with
`sqlx::QueryBuilder`. The full configuration-key filter set is supported
so a results row can link to exactly its runs. `limit` default 50, max
200. Alternative considered: offset paging - skips or repeats rows under
inserts and gets slower with depth.

**Run summary is a separate DTO from the full run response.** `RunSummary`
carries list columns only (no JSON blobs or env capture), joined to
`model_assets` for `original_name` in the list query rather than N
lookups.

**Expanded `RunResponse`: flat, additive, same field names as the `Run`
domain struct, JSON columns decoded to JSON values.** Units go in schema
descriptions, not names. `command_args`, `input_parameters`, `env_vars`
are `serde_json::Value`. Existing fields are untouched. Alternative
considered: nested `device`/`clocks`/`build`/`results` objects - deferred
by the user ("flat for now"); the dashboard groups at render time.

**Column collapsing is a client-side presentation rule.** For the visible
rows, each key column whose values are all equal is hidden and its value
listed in a "shared configuration" line; a show-all toggle overrides.
Computed from the rows already in hand; no backend involvement.

**`api_version` bumps to `1.1`.** Additive operations and fields.

**Listen address becomes configurable (discovered during implementation).**
The bind address was hard-coded to `0.0.0.0:3000`, and on the development
machine port 3000 is already held by an unrelated service, which made the
end-to-end verification impossible without a change. `Config` gains
`listen_addr` from an optional `LISTEN_ADDR` env var, validated as a
socket address, with the default unchanged. Small, additive, and needed
for the dev proxy story to be usable on this machine; recorded here rather
than absorbed silently.

**Static serving: `tower-http` `ServeDir` with `index.html` fallback,
gated on `DASHBOARD_DIST`.** `Config.dashboard_dist: Option<PathBuf>`;
startup fails if set but not a readable directory containing
`index.html`. `http::router` attaches
`ServeDir::new(dir).not_found_service(ServeFile::new(dir/index.html))` as
`fallback_service` only when configured, so API/health/docs routes always
win and the unset case is byte-for-byte today's behavior. Alternative
considered: `rust-embed` - couples the Rust build to a Bun build with no
release pipeline to justify it.

**Existing contract tests flip rather than get deleted.** The
`GET /api/v1/runs` absence test becomes a presence test for both new
operations with their parameters; the "unexposed fields" test becomes an
exact-field-set assertion on `RunResponse`.

**Frontend toolchain: Bun-native, starting from `bun init --react=tailwind`
and trimmed.** Keep `Bun.serve` with the HTML import and HMR, a
`build.ts` calling `Bun.build` with `bun-plugin-tailwind` (never bare
`bun build`), `bunfig.toml` with the plugin for `serve.static` and a
happy-dom test preload, and `bun test`. Remove the template's sample API
routes and components. Pin `typescript@5` and drop the template's `^7`
peer entry. Alternative considered: Vite-on-Bun - unnecessary, the spike
showed no blocker.

**Dev-time API access: proxy inside the Bun dev server, no CORS.** `dev.ts`
registers `/api/*` and `/health` routes that `fetch` `BACKEND_URL`
(default `http://127.0.0.1:3000`) with method, headers, and `req.body`
forwarded, `redirect: "manual"`, returning `new Response(upstream.body,
{status, headers})` so bodies stream and `Content-Disposition` survives
(verified in the spike). All other paths serve the HTML import. Default
port 3001.

**Stack inside the app.** React 19 with TypeScript 5 (`strict`),
`react-router` v7 declarative mode for the three routes,
`@tanstack/react-query` for fetching, caching, retry, and loading/error
state (`useInfiniteQuery` for the run list), `openapi-fetch` with types
from `openapi-typescript`, Tailwind v4 via the plugin, no component
library, light theme only. Each is the smallest well-known tool for its
job. Alternatives considered: TanStack Router (heavier for three routes),
hand-written fetch wrappers (would drift from the contract), shadcn
(a copied component tree for three pages), dark/system theme (user chose
light only).

**Generated types are checked in, with a drift check.** `bun run
generate-api` runs `openapi-typescript ../openapi/openapi.json -o
src/api/schema.d.ts`. `bun run check` runs `tsc --noEmit` then
`scripts/check-api-drift.ts`, which regenerates to a temp file and diffs.
Mirrors the Rust `openapi_drift` test.

**Time display.** Tables render local time via `Intl.DateTimeFormat`, with
the raw UTC RFC 3339 string in a `title` attribute; the detail view shows
both. Model label is `original_name` with a trailing `.pte` stripped.

**Inline artifact preview bound.** Inline view only when `media_type`
starts with `text/` and `size_bytes` ≤ 256 KiB; otherwise download only.
Unavailable artifacts render without links. `download_url`/`content_url`
from the API are used as-is.

**Package layout.**

```text
dashboard/
├── package.json          # scripts: dev, build, check, test, generate-api
├── bunfig.toml           # [serve.static] tailwind plugin; [test] happy-dom preload
├── tsconfig.json         # strict, bundler resolution, react-jsx
├── dev.ts                # Bun.serve: HTML route + /api,/health proxy (port 3001)
├── build.ts              # Bun.build with bun-plugin-tailwind -> dist/
├── scripts/check-api-drift.ts
├── tests/happydom.ts     # GlobalRegistrator.register()
└── src/
    ├── index.html        # <script src="./main.tsx">, <link href="./app.css">
    ├── main.tsx          # React root, router, QueryClientProvider
    ├── app.css           # @import "tailwindcss"
    ├── api/
    │   ├── schema.d.ts   # generated by openapi-typescript (checked in)
    │   ├── client.ts     # openapi-fetch createClient<paths>({ baseUrl: "" })
    │   └── errors.ts     # normalize result -> data | {code,message} | unreachable
    ├── lib/              # format.ts, filters.ts, collapse.ts (+ .test.ts)
    ├── pages/            # ResultsPage, RunsPage, RunDetailPage
    └── components/       # ResultsTable, SharedConfigLine, FilterBar, RunsTable,
                          # FieldGroup, ArtifactCard, ErrorState, EmptyState
```

`dashboard/dist/` and `dashboard/node_modules/` are gitignored;
`dashboard/bun.lock` is committed.

## Risks / Trade-offs

- **[Results fold loads every matching run's key row into memory]** →
  Only ten small columns per run; at 100k runs this is a few MB and
  sub-second. If it ever matters, add a `since` window before considering
  SQL-side grouping.
- **[500-row cap hides configurations]** → `truncated: true` is surfaced
  as a notice; six models × few devices × commits keeps this in the
  hundreds for a long time.
- **[Commit ordering falls back to run time until the runner fills git
  metadata]** → Acceptable for a single developer; the fallback is
  documented on the page as "ordered by first run" when timestamps are
  absent.
- **[`bun-plugin-tailwind` bundles its own Tailwind version]** → Pin the
  plugin version; the `tailwindcss` package is not the compiler that runs
  and is omitted from dependencies to avoid confusion.
- **[`openapi-typescript` requires TypeScript 5]** → Pin
  `typescript@5.x` and remove the template's `^7` peer entry; `bun run
  check` will fail loudly if this regresses.
- **[Generated `schema.d.ts` goes stale when the backend changes]** → The
  drift script and a README note ("after `cargo run --bin gen-openapi`,
  run `bun run generate-api`").
- **[Keyset cursor encodes `started_at` text]** → Compared against the
  same RFC 3339 text SQLx writes; the cursor is opaque so the encoding can
  change; a test pages with interleaved inserts.
- **[`ServeDir` fallback could shadow a future API 404]** → The fallback is
  only reached when no router route matched; documented in `docs/api.md`.
- **[Two toolchains in one repo]** → Bun is only required for the
  dashboard; `cargo test` stays Bun-free. README states this explicitly.

## Migration Plan

One additive SQLite migration (three nullable columns); existing rows are
unaffected. Backend rollout is additive: deploy the new binary, existing
clients see new fields and operations only. `DASHBOARD_DIST` is opt-in.
Rollback is a binary revert plus the migration's `down` (drops the three
columns). The dashboard is a new package with no prior users.

## Open Questions

- Whether the run detail view should link to the model's registry entry
  as a separate page - deferred; the detail view shows the model summary
  inline for now.
