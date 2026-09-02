## Why

The runs table hard-codes an Android phone as the host: BSP version, SUMD
driver, battery state, temperatures, thermal throttling, and the three pinned
clocks are all required columns. The first real measurements we have,
though, come from three Linux workstations (RTX 4070 Ti SUPER, Radeon 780M,
Arc B580) running the same ExecuTorch `llama_main` runner on the Vulkan
backend - valuable comparison points that cannot be recorded at all today.
The database also has no real data yet; the only rows are the e2e example's
synthetic runs, and there is no separation between a database you can
safely fill with fakes for dashboard work and the one that holds real
numbers.

## What Changes

- Add a `platform` discriminator (`android` | `linux`) to every run. The
  Android snapshot columns become nullable and required only on `android`
  rows; new nullable `host_*` columns (OS, kernel, CPU model, CPU count,
  memory, accelerator, accelerator driver) are required only on `linux`
  rows. A CHECK constraint enforces that each platform carries exactly its
  own snapshot. `device_serial` stays as the universal host identity and
  holds the hostname on Linux. The table is rebuilt by migration; existing
  rows carry over as `android`.
- Let `executable_sha256` be null when the runner binary's identity was
  not preserved, rather than requiring a fabricated value.
- Add a `device_class` (`internal` | `external`) to every run. Internal
  devices are lab phones under full control and must carry the rigorous
  Android snapshot (BSP, SUMD driver, pinned clocks, battery,
  temperatures, throttling, uptime). External hosts - retail, unrooted
  phones and every Linux box - record what they can report (build,
  kernel, SoC, GPU and driver, memory, model name) and leave the rest
  null. Everything measured so far is external; internal devices come
  later.
- Model the snapshot in Rust as a `HostState` enum (`Android` /
  `Linux`), so a run cannot be constructed with the wrong platform's
  fields.
- Extend the configuration key, run listing, run detail, results, and
  facets with `platform` and (Linux) `host_accelerator`; Android-only
  fields become nullable in the API. Add `platform` and
  `host_accelerator` filters.
- Dashboard: platform and host filters, a "Host" column that reads as
  device serial or hostname, accelerator alongside driver/BSP columns,
  and a platform-appropriate host group on the run detail page.
- Add an `import-observer-log` binary that turns a `llama_main`
  `PyTorchObserver` log plus a manifest of externally known metadata into
  runs, artifacts, and model registrations, idempotently. Check the raw
  Linux logs and their manifests into `imports/`.
- Split storage into two profiles: `.env` (the mock database under
  `data/dev/`, seeded by `examples/seed_mock_data.rs`) and `.env.real`
  (the real database under `data/real/`). `just` recipes take the profile.

## Capabilities

### Modified Capabilities
- `benchmark-schema`: host state is platform-specific; executable hash is
  nullable.
- `ingestion-service`: results, listing, and single-run responses carry
  platform and Linux host fields; new filters and facets.
- `benchmark-dashboard`: platform-aware results, runs, and run detail
  views.

The two retail phones the first Android runs came from (Pixel 7a, Galaxy
S24) cannot pin clocks and have no SUMD or BSP control; their prefill
numbers are still the point of the exercise, so the schema must accept
them without inventing values.

## Impact

- **Migration** rebuilds `runs`; the down migration drops Linux rows and
  rows without an executable hash, which the old schema cannot hold.
- **API**: additive fields plus nullability changes on previously
  non-null Android fields (`bsp_version`, `sumd_driver_version`, clocks,
  temperatures, `battery_charging`, `thermal_throttling`,
  `executable_sha256`). `api_version` bumps to `1.2`.
- **Existing data**: every current row is an Android run and is preserved.
- **Not in scope**: a Linux collector. The import path is for existing
  logs; a runner that writes runs over HTTP remains future work.
