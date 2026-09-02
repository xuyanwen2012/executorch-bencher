# executorch-bencher

Backend (Rust, Axum + SQLite) and dashboard (Bun + React + Tailwind) for
collecting and viewing LLM benchmark runs from Android phones and Linux
hosts.

**No authentication.** Neither the API nor the dashboard authenticates
anything; the service is meant for a single workstation or a trusted lab
network. Do not expose it to the internet. The one route that takes a
server-side path, `POST /api/v1/models/register`, is confined to `.pte`
files beneath `MODEL_REGISTER_ROOTS` (default: the model root) so an
unauthenticated caller cannot probe or hash arbitrary server files.

Runs come from two kinds of host: **Android phones** and **Linux boxes**.
Every run also has a **device class**. `internal` devices are lab phones
under full control (rooted, SUMD/BSP/clock pinning) and must carry the
rigorous snapshot: BSP, SUMD driver, battery, temperatures, throttling,
uptime, and pinned GPU/MIF/INT clocks. `external` hosts are retail,
unrooted phones and every Linux box; they record what they can report
(model, OS build, kernel, CPU/SoC, memory, accelerator and driver) and the
rest stays null. Everything measured so far is external. The database
CHECK and the Rust `HostState`/`DeviceClass` types enforce the rule.

## Two databases: mock and real

| Profile | Env file | Storage root | Contents |
|---|---|---|---|
| `dev` | `.env` | `data/dev/` | **Mock** data for dashboard work. Seed with `just seed-mock`. |
| `real` | `.env.real` | `data/real/` | **Real** measurements only: imported logs (`just import-all`) and, later, collector uploads. |

Neither env file is loaded automatically; the `just` recipes take the
profile as their first argument (`just serve-backend real`) and the
import/seed recipes are pinned to their profile. The mock seeder refuses to
run against anything but `data/dev/`.

## Running the backend

```sh
set -a; source .env; set +a      # or .env.real; neither is loaded automatically
cargo run
just serve-backend               # dev profile on 127.0.0.1:3100
just serve-backend real          # real profile
```

`DATABASE_URL` is required. `LISTEN_ADDR` (default `0.0.0.0:3000`) and
`DASHBOARD_DIST` (unset by default) are optional - see `.env`.

## Recording runs from a collector

A benchmark script records each repetition over HTTP: upload the captured
artifacts (`POST /api/v1/artifacts`), find the model asset by hash
(`GET /api/v1/models?sha256=...`), then post one complete run
(`POST /api/v1/runs`). The request mirrors the run response, validation
errors name the field, and a retried submission of the same run `id` is a
`409 conflict` rather than a duplicate. `GET /api/v1/events` streams
`run.created` / `artifact.created` / `model.registered` notifications so
the dashboard refreshes while a session runs. See `docs/collector.md` and
the dependency-free reference `examples/post_run.py`:

```sh
./llama_main ... 2>/dev/null | grep PyTorchObserver \
  | python3 examples/post_run.py --backend http://127.0.0.1:3100 \
      --model /mnt/linux-share/models/.../model.pte --prompt-file prompt.txt \
      --argv "..." --git-sha <sha> --repetition 0
```

## Importing benchmark logs

Logs that predate a collector: existing `llama_main` logs (lines of
`=== <tag> rep<N> ===` followed by `PyTorchObserver {json}`) are imported
into the real database with a manifest that supplies everything the log
lacks - host, git provenance, model and prompt identities, the command
template, and which values were *not* captured:

```sh
just import-log imports/linux-vulkan-2026-09-01/manifests/04-ubuntu-lts-gpu-prefill-2048.json
just import-all                  # every manifest under imports/
```

Imports are idempotent (keyed by log SHA-256, tag, and repetition). See
`imports/README.md`.

## Dashboard

The dashboard lives in `dashboard/` and needs only [Bun](https://bun.sh)
(1.4 or newer); no Node.js toolchain. `cargo test` never needs Bun.

```sh
cd dashboard
bun install
bun run dev          # http://localhost:3001, proxies /api and /health to BACKEND_URL (default http://127.0.0.1:3000)
bun run build        # writes dist/ (index.html + hashed assets); never use bare `bun build`
bun run check        # tsc --noEmit + generated-API-types drift check
bun test             # unit tests
bun run generate-api # regenerate src/api/schema.d.ts from ../openapi/openapi.json
```

Pages: **Results** (`/`, one row per benchmark configuration - platform,
host, model, commit, and the platform's own dimensions - with
median/min–max/n prefill and decode tok/s over succeeded runs, newest
commit first, columns that do not vary collapsed into a shared line),
**Runs** (`/runs`, newest-first list with filters and load-more paging),
and **Run detail** (`/runs/<id>`, every recorded field plus the model and
attached artifacts with view/download).

After changing a backend route or schema, regenerate both sides of the
contract and commit the results:

```sh
cargo run --bin gen-openapi          # openapi/openapi.json
(cd dashboard && bun run generate-api)  # dashboard/src/api/schema.d.ts
```

`cargo test` (the drift test) and `bun run check` (the TypeScript drift
script) each fail when their side is stale.

### Serving the dashboard from the backend

Build the dashboard and point the backend at the output:

```sh
(cd dashboard && bun run build)
DASHBOARD_DIST=dashboard/dist cargo run
```

The backend then serves the app at `/` with a single-page-app fallback
(client-side routes such as `/runs/<id>` reload correctly). API, `/health`,
`/docs`, and `/openapi.json` always take precedence. Startup fails with a
clear error if `DASHBOARD_DIST` is set but is not a readable directory
containing `index.html`. The dashboard has no authentication; it is meant
for a single workstation.

## HTTP API documentation

Interactive Swagger UI is served at `/docs`; the raw generated OpenAPI
document is at `/openapi.json`. A checked-in copy lives at
[`openapi/openapi.json`](openapi/openapi.json) - regenerate it with
`cargo run --bin gen-openapi` after changing a route or schema. See
`docs/api.md` for versioning, client generation, and which parts of the API
are implemented versus documented as a gap.

## Local database

The backend uses an embedded SQLite database file - no external service is
required. Each profile keeps its database and all other backend-managed
storage under its own data root (`DATA_ROOT` in the env file; `data/dev/`
for the mock profile, `data/real/` for the real one):

```text
data/<profile>/
├── benchmarks.sqlite3
├── artifacts/sha256/<prefix>/<sha256>   # content-addressed artifact blobs
├── models/sha256/<prefix>/<sha256>      # managed-mode model copies (future)
├── temporary/                           # in-flight uploads before dedup
└── trash/                               # reserved for future safe deletion
```

Each root can be overridden independently of `DATA_ROOT` via `ARTIFACT_ROOT`,
`MODEL_ROOT`, `TEMPORARY_DIR`, and `TRASH_DIR` in `.env`. All four are
created automatically on startup; startup fails with a clear error naming
the specific root if any of them can't be created or written to.
`MODEL_REGISTER_ROOTS` (colon-separated, default `MODEL_ROOT`) lists the
directories `POST /api/v1/models/register` may register `.pte` files from;
the real profile points it at the NFS model share. See `docs/storage.md`
for the full storage design (deduplication, model registration, backups,
retention).

## Offline SQLx query cache

SQLx checks `query!`/`query_as!` macro calls against a real database at
compile time. To let `cargo check`/`cargo build` succeed without a live
database (`SQLX_OFFLINE=true`), regenerate the committed `.sqlx/` cache
whenever a query changes:

```sh
export DATABASE_URL=sqlite://data/benchmarks.sqlite3
cargo sqlx migrate run
cargo sqlx prepare
```

Commit the resulting `.sqlx/` directory alongside the code change.
