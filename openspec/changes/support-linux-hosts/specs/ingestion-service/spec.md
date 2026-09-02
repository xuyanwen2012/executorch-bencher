## MODIFIED Requirements

### Requirement: Service exposes grouped benchmark results
The system SHALL expose `GET /api/v1/results` returning one row per
benchmark configuration. The configuration key SHALL be the platform, the
device class, the host identity (`device_serial`), the model asset, the git commit SHA, the
dirty flag, the prompt SHA-256, and the platform's own dimensions: SUMD
driver version, BSP version, and the three pinned clocks on Android; the
accelerator on Linux. Fields belonging to the other platform SHALL be null
on a row. Each row SHALL report median, minimum, maximum, and sample count
of prefill and decode throughput over succeeded runs, the total run count,
the count of runs that did not succeed, the count of correctness failures,
the count of runs that reported thermal throttling (runs that did not
capture it count as not throttled), the earliest and latest run start, and
the model's original name and commit metadata. Each row SHALL also carry the device model name when recorded. The
response SHALL include `facets`: the distinct platforms, device classes,
host identities, models, branches, SUMD driver versions, BSP versions, and
accelerators across all runs, ignoring the active filters. Filters SHALL
be exact-match on `platform`, `device_class`, `device_serial`,
`model_asset_id`, `git_commit_sha`, `git_branch`, `git_dirty`,
`sumd_driver_version`, `bsp_version`, `host_accelerator`,
and `prompt_sha256`. At most 500 rows are returned, with a `truncated`
flag.

#### Scenario: Repetitions collapse into one row with a median
- **WHEN** five succeeded runs of one configuration recorded prefill
  throughputs of 100, 110, 120, 130, and 900 tokens per second
- **THEN** the results contain one row for that configuration with a prefill
  median of 120, minimum 100, maximum 900, and n of 5

#### Scenario: Linux hosts group by hostname and accelerator
- **WHEN** runs exist for one model and commit on Linux host `box-a` with
  two different accelerators and on host `box-b` with one
- **THEN** the results contain three Linux rows, each with its accelerator
  and null SUMD driver, BSP, and clock fields, and filtering on
  `platform=linux` and `host_accelerator` narrows to the matching rows

#### Scenario: Facets include platforms and accelerators
- **WHEN** both Android and Linux runs exist
- **THEN** `facets.platforms` lists `android` and `linux` and
  `facets.host_accelerators` lists each distinct accelerator

#### Scenario: An unknown platform filter is rejected
- **WHEN** a client passes `platform=ios`
- **THEN** the service responds 400 with the `invalid_request` envelope

### Requirement: Service exposes a paginated, filterable run listing
The system SHALL expose `GET /api/v1/runs` returning run summaries newest
first with opaque keyset pagination. Each summary SHALL include the
platform, device class, host identity, device model, commit SHA and dirty
flag, branch, the platform's
key dimensions (SUMD driver and BSP on Android; accelerator on Linux, the
other platform's fields null), the model reference, exit status,
correctness result, prefill and decode throughput, and thermal throttling
(null when not captured). Filters SHALL be exact-match on `platform`,
`device_class`, `device_serial`, `model_asset_id`, `git_commit_sha`, `git_branch`,
`git_dirty`, `sumd_driver_version`, `bsp_version`, `gpu_clock_mhz`,
`mif_clock_mhz`, `int_clock_mhz`, `host_accelerator`, `prompt_sha256`,
`exit_status`, and `correctness_result`, combined conjunctively.

#### Scenario: Listing filters by platform and accelerator
- **WHEN** one Linux run and no Android runs exist and a client requests
  `platform=linux`, then `platform=android`, then
  `host_accelerator=<that run's accelerator>`
- **THEN** the first and third responses contain exactly that run and the
  second is empty

#### Scenario: A full configuration key selects exactly one configuration's runs
- **WHEN** a client passes every configuration-key filter for one
  configuration
- **THEN** the response contains exactly the runs of that configuration

### Requirement: Single-run responses expose the complete recorded run
`GET /api/v1/runs/{id}` SHALL return every recorded field of the run as
top-level fields named as stored, with units in their documentation. The
response SHALL include `platform`, `device_class`, and `device_model`;
the Android lab snapshot fields, null where not recorded; the host
description fields (`host_os`, `host_kernel`, `host_cpu_model`,
`host_cpu_count`, `host_memory_bytes`, `host_accelerator`,
`host_accelerator_driver`), null where not recorded; and
`executable_sha256`, null when unknown.

#### Scenario: A Linux run reads back its host fields and null Android fields
- **WHEN** a client fetches a Linux run
- **THEN** the response has `platform` `linux`, the host fields as recorded,
  and null `bsp_version`, `sumd_driver_version`, `battery_charging`,
  temperatures, and clocks

#### Scenario: A retail phone's run reads back its description and null lab fields
- **WHEN** a client fetches an external Android run
- **THEN** the response has `device_class` `external`, the device model,
  build, kernel, SoC, GPU, and driver as recorded, and null BSP, SUMD,
  clocks, battery, temperatures, uptime, and throttling

#### Scenario: Existing fields are unchanged
- **WHEN** a client fetches an Android run recorded before this change
- **THEN** every previously documented field is present with its previous
  name, value, and shape, and the Linux host fields are null
