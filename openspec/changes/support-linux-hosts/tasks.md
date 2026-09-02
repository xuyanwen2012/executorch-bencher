## 1. Schema and domain

- [x] 1.1 Add migration `add_run_platform_and_linux_host` rebuilding `runs` with `platform`, nullable Android columns, `host_*` Linux columns, a platform CHECK, and nullable `executable_sha256`; down migration carries Android rows back and drops the rest; verify `tests/migrations.rs` round-trips an Android run and drops a Linux run.
- [x] 1.2 Add `domain::Platform`; add `runs::{AndroidDeviceState, LinuxHostState, HostState}` and replace the flat device/clock fields on `NewRun`/`Run` with `host`; make `executable_sha256` optional; update `insert_run`, `row_to_run`, and the list summary; verify existing round-trip tests plus a Linux round trip and the CHECK constraint tests in `tests/runs_lifecycle.rs`.

- [x] 1.3 Add migration `add_device_class_and_relax_external_hosts` (device_class, device_model, host description on Android, relaxed CHECK per device class); add `domain::DeviceClass` and split `AndroidDeviceState` into optional description fields plus `Option<AndroidLabConfig>`; reject internal devices without the full snapshot in `insert_run`; verify CHECK tests for external phones and the internal/external API round trips.

## 2. Read surface

- [x] 2.1 Extend `ConfigKey`, `ResultsFilter`, `RunListFilter`, and `Facets` with platform and accelerator; make the Android key fields optional; count throttling only when captured; verify unit tests for Linux grouping and facets.
- [x] 2.2 Extend `RunResponse`, `RunSummaryResponse`, `ResultRowResponse`, `FacetsResponse`, and both parameter structs; document nullability per platform; bump `api_version` to `1.2`; regenerate `openapi/openapi.json`; verify `tests/openapi_contract.rs`, `tests/runs_api.rs`, and `tests/results_api.rs` including new Linux cases.

## 3. Dashboard

- [x] 3.1 Regenerate `schema.d.ts`; add `platform` and `host_accelerator` to the results and runs filter keys, queries, and configuration links.
- [x] 3.2 Results page: platform and host columns, accelerator column, absent rendering for the other platform's fields, platform and accelerator filters from facets.
- [x] 3.3 Runs page: platform and accelerator filters; driver/accelerator column reads whichever the platform has.
- [x] 3.4 Run detail: platform-dependent host group; absent executable hash.
- [x] 3.5 `bun run check` and `bun test` pass.

## 4. Import and data profiles

- [x] 4.0 Extend the importer for Android manifests (`host.platform`, `host.device_class`, on-device model paths) and for failed repetitions (`failures`, estimated start times); import `imports/android-vulkan-2026-09-01/manifests/*` into the real database.
- [x] 4.1 Add `src/bin/import_observer_log.rs` (manifest + `PyTorchObserver` log → model registration, prompt and stdout artifacts, idempotent run inserts, `skip_tags`); verify by importing every manifest under `imports/linux-vulkan-2026-09-01/manifests/` into the real database and checking counts and medians against the logs.
- [x] 4.2 Check in the raw logs, prompt, scripts, and manifests under `imports/`.
- [x] 4.3 Split `.env` (dev, `data/dev/`) and `.env.real` (`data/real/`); move the existing mock data to `data/dev/`; `just` recipes take a profile, plus `seed-mock`, `import-log`, `import-all`, `integrity`.
- [x] 4.4 Add `examples/seed_mock_data.rs` seeding Android and Linux fake runs into the dev database, skipping if already seeded.
- [x] 4.5 Update README, CLAUDE.md, and `docs/api.md` for platforms, profiles, and the import path.
