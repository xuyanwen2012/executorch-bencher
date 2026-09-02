## Context

The schema, API, and dashboard all assume an Android phone. Three Linux
boxes have produced real `llama_main` results that we want as the first
real data. The constraints: keep the spec's "immutable snapshot on the
run's own row" principle, keep every existing Android field and its
validation, keep the results key exact (no cross-platform rows collapsing
together), and never invent values we do not have.

## Goals / Non-Goals

**Goals**
- One `runs` table for both platforms, with the database and the Rust
  types agreeing on which fields each platform carries.
- Import the existing Linux logs faithfully, including what was *not*
  captured.
- A mock database for dashboard work that can never be confused with the
  real one.

**Non-Goals**
- A Linux collector or an HTTP write path for runs.
- Per-platform correctness validation or thermal capture on Linux.
- Modelling multi-GPU hosts beyond "which accelerator ran this".

## Decisions

### One table, a discriminator, and a CHECK - not a JSON blob or per-platform tables

`platform TEXT NOT NULL CHECK (platform IN ('android','linux'))`, with the
Android-only columns made nullable and new nullable `host_*` columns for
Linux. A single CHECK requires the Android set on `android` rows and
forbids the Linux set, and vice versa. Alternatives considered:

- *JSON `platform_state` column*: smallest migration, but loses typed
  filters, indexes, and the CHECK constraints the spec relies on.
- *Separate `android_runs` / `linux_runs` tables*: doubles every query and
  the results fold for no benefit; the shared columns dominate.

`device_serial` is kept (not renamed to `host_id`) as the universal
identity to avoid churning the API, dashboard, and spec; on Linux it holds
the hostname.

### `HostState` enum in Rust

`NewRun`/`Run` carry `host: HostState` with `Android(AndroidDeviceState)`
and `Linux(LinuxHostState)` variants instead of a dozen `Option` fields.
A caller cannot build a Linux run with a BSP version, and reading a row
back reconstructs the variant from `platform`, failing loudly if a
required column is null (which the CHECK makes impossible). The HTTP
responses stay flat (nullable fields plus `platform`) because that is what
`openapi-typescript` and the dashboard consume best.

### Configuration key

Adds `platform` and, for Linux, `host_accelerator`; the Android dimensions
(SUMD driver, BSP, clocks) become `Option`s that are `None` on Linux. Two
Linux hosts with the same hostname but different accelerators are distinct
configurations; the accelerator is what the backend actually executed on.

### `device_class`: rigor is a property of the device, not the platform

The Android snapshot was designed for lab phones. The first real Android
data comes from retail, unrooted phones (`uid=2000(shell)`, no `su`), which
cannot report BSP or SUMD versions and cannot pin GPU/MIF/INT clocks. Their
prefill throughput is still what we want to compare against the Linux
boxes. So the rule becomes: an `internal` device must carry the full lab
snapshot (the previous CHECK, unchanged); an `external` device carries
whatever it can report. In Rust, `AndroidDeviceState` holds the
descriptive, always-optional part (build, kernel, SoC, GPU, driver,
memory, uptime, battery, temperatures, throttling) and an
`Option<AndroidLabConfig>` with the five lab-only fields; `insert_run`
refuses an internal device whose snapshot is incomplete, and the database
CHECK enforces the same rule per `(platform, device_class)`, with the lab
fields all-or-none on external phones. The `host_*` columns become usable
on Android too, so a phone's build, kernel, SoC, and GPU live in the same
columns as a Linux box's OS, kernel, CPU, and accelerator. `device_class`
joins the configuration key, filters, and facets. Existing Android rows
(all synthetic, all complete) migrate as `internal`.

### `executable_sha256` becomes nullable

Two of the three Linux hosts had their runner rebuilt after the benchmark
and the original binary was not preserved. Storing the rebuilt binary's
hash would be a lie; a sentinel would be a worse lie. Null with the reason
recorded in `input_parameters.executable_note` is the honest record. The
spec requirement changes from "record the hash" to "record the hash when
known, null otherwise, never a placeholder".

### Failed repetitions in imported logs

A marker with no observer line is a rep that produced nothing. The
manifest's `failures` list says what happened (`crashed` for the Pixel 7a
reboot on the first 8B attempt, `infrastructure_error` for the attempts
made while it was offline) and the importer records those as runs with
zero measurements, so the configuration row shows them as not succeeded
instead of silently missing. When no timestamp exists for a failed rep,
`started_at` is the end of the last observed rep and the row is flagged
`started_at_estimated` in `input_parameters`.

### Import via manifest + log, idempotent

The observer log carries only throughput, token counts, and epoch-ms
timestamps. Everything else (host, git, model paths and hashes, prompt,
command template) comes from a JSON manifest next to the log. The importer
registers models by looking up the manifest's SHA-256 first and hashing
the file only if unknown, stores the prompt and each rep's captured stdout
line as artifacts, and records `input_parameters.import = {log_sha256,
tag, rep, manifest}`; a run with the same triple already present is
skipped. Superseded reps (an 8B pair measured while VRAM was contended)
are listed in `skip_tags` with the reason rather than imported into the
same configuration's median.

### Two database profiles

`.env` is the dev/mock profile (`data/dev/`), `.env.real` the real one
(`data/real/`). The `just` recipes take a profile argument and the import
recipes hard-wire `.env.real`; the mock seeder hard-wires `.env`. Storage
roots are already fully configurable, so this is configuration, not code.

## Risks / Trade-offs

- **Nullability change on existing API fields** is visible to clients; the
  dashboard is the only client and is updated in the same change.
- **Down migration is lossy** for Linux rows by necessity; documented in
  the migration and tested.
- **Model registration over NFS** hashes multi-GB files once; the importer
  short-circuits on a known SHA-256 so repeated imports do not re-hash.

## Migration Plan

1. Apply the rebuild migration (automatic on startup).
2. Regenerate `openapi/openapi.json` and the dashboard types.
3. Move the existing `data/` contents to `data/dev/`; create `data/real/`.
4. Run `just import-all` against `.env.real`.
5. Run `just seed-mock` against `.env`.
