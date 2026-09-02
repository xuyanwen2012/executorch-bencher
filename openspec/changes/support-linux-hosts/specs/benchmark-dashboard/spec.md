## MODIFIED Requirements

### Requirement: Results page shows one row per benchmark configuration
The results page SHALL show one row per configuration returned by
`GET /api/v1/results`, newest commit first, with the platform, the host
(device serial or hostname), the model, and the platform's own dimensions
as key columns: SUMD driver, BSP, and clocks for Android rows; accelerator
for Linux rows. A dimension the row's platform does not have SHALL render
as absent, not blank. The page SHALL offer filters on platform, host,
model, branch, SUMD driver, BSP, accelerator, and working-tree state, with
select options taken from the response's facets.

#### Scenario: Android and Linux rows appear together
- **WHEN** both platforms have configurations for one commit
- **THEN** each row shows its platform and host, Android rows show driver,
  BSP, and clocks, Linux rows show the accelerator, and the other
  platform's columns read as absent

#### Scenario: Filtering by platform
- **WHEN** the user selects `linux` in the platform filter
- **THEN** only Linux rows remain and the URL carries `platform=linux`

### Requirement: Run detail view shows the complete run record
The run detail page SHALL group every field of `GET /api/v1/runs/{id}`
and SHALL render the host group according to the run's platform: "Device
state" and "Performance configuration" for an Android run, "Host" (OS,
kernel, CPU, memory, accelerator, driver) for a Linux run. An unknown
executable hash SHALL render as absent.

#### Scenario: Opening a Linux run
- **WHEN** the user opens a Linux run by URL
- **THEN** the page shows a Host group with the OS, kernel, CPU model,
  accelerator, and driver, and no Android device or clock group
