## MODIFIED Requirements

### Requirement: Host state is captured as a platform-specific immutable snapshot
The system SHALL record, on each run's own immutable row, which platform
the run executed on (`android` or `linux`), whether the host is an
`internal` lab device under full control or an `external` one (a retail
phone or any Linux box), and the host state at the time of the run,
identified solely by a host identity string (`device_serial`: the device
serial on Android, the hostname on Linux) plus an optional device model
name, rather than referencing a mutable "current host" record. Every run
MAY carry a host description: OS release (Android build on phones),
kernel release, CPU or SoC model, CPU count, memory size, the accelerator
the runtime backend executed on, and its driver version. An `internal`
Android run SHALL additionally include BSP version, SUMD driver version,
device uptime, battery charging state, temperature at start, maximum
temperature observed, thermal throttling, and the three pinned clocks. An
`external` Android run MAY include any of those, with BSP, SUMD driver,
and the three clocks present all together or not at all. A Linux run
SHALL include OS release, kernel release, CPU model, and accelerator, and
SHALL carry no BSP, SUMD driver, battery, temperature, or clock fields.
The database SHALL reject a row whose snapshot columns do not match its
platform and device class.

#### Scenario: Same device produces two different snapshots over time
- **WHEN** a device's BSP version is upgraded between two benchmark sessions
- **THEN** runs from the first session retain the old BSP version in their own
  row and runs from the second session retain the new BSP version in their
  own row, and neither row's captured device fields change after creation

#### Scenario: Snapshot captures reproducibility-relevant device fields
- **WHEN** an internal Android run is recorded
- **THEN** it includes, at minimum, the device serial, BSP version, SUMD
  driver version, device uptime in seconds, battery charging state,
  temperature at start, maximum temperature observed, and whether thermal
  throttling was detected; a write missing any of them is rejected

#### Scenario: A retail phone records what it can report
- **WHEN** an external Android run is recorded with the device model,
  Android build, kernel, SoC, GPU, and driver, and no BSP, SUMD, clocks,
  battery, or temperatures
- **THEN** the system accepts the write and reads the unavailable fields
  back as null

#### Scenario: Lab-only fields are all-or-none on an external phone
- **WHEN** an external Android run is recorded with a pinned GPU clock but
  no BSP version
- **THEN** the system rejects the write

#### Scenario: A Linux run captures its host and accelerator
- **WHEN** a Linux run is recorded with hostname, OS release, kernel, CPU
  model, and accelerator name
- **THEN** the system stores them, reads them back unchanged, and reads every
  Android-only field back as null

#### Scenario: A platform's snapshot cannot carry the other platform's fields
- **WHEN** a write supplies a Linux run with a pinned GPU clock, an unknown
  platform value, or an unknown device class
- **THEN** the system rejects the write

#### Scenario: Existing Android runs survive the migration
- **WHEN** the platform and device-class migrations are applied to a
  database that already contains runs
- **THEN** every existing run remains readable as an `android`, `internal`
  run with its snapshot unchanged

### Requirement: Performance configuration is captured per run
The system SHALL record, for each internal Android run, exactly three
pinned clocks: GPU clock, MIF (memory-interface) clock, and INT
(interconnect) clock, each expressed in MHz as an integer. No CPU-cluster,
NPU, DSP, or general memory-frequency fields are part of this requirement.
External Android runs MAY record them; Linux runs SHALL record no pinned
clocks; absent clocks are null.

#### Scenario: Performance configuration defaults to documented values
- **WHEN** an Android run is recorded without an explicit GPU, MIF, or INT
  clock value
- **THEN** the system records the documented default for that clock (980 MHz
  for GPU, 5333 MHz for MIF, 934 MHz for INT) rather than leaving it unset

#### Scenario: Clock values must be positive
- **WHEN** an Android run is recorded with a zero or negative GPU, MIF, or
  INT clock value
- **THEN** the system rejects the write

### Requirement: Build and workload identity are captured per run
The system SHALL record, for each run, the git commit SHA, whether the
working tree was dirty, the executable's SHA-256 hash when it is known, a
reference to the registered model asset exercised by the run, the prompt
text file's SHA-256 hash, the input token count, and the output token
count. The executable hash SHALL be null, never a placeholder, when the
binary's identity was not preserved.

#### Scenario: SHA-256 values are validated at the application boundary
- **WHEN** a run is recorded with an executable or prompt hash
- **THEN** the system accepts the value only if it is exactly 64 lowercase
  hexadecimal characters, and rejects it otherwise

#### Scenario: An unknown executable hash is recorded as null
- **WHEN** a run is recorded without an executable hash
- **THEN** the system accepts the write and reads the hash back as null

#### Scenario: Token counts must be nonnegative
- **WHEN** a run is recorded with a negative input or output token count
- **THEN** the system rejects the write

#### Scenario: A run's model reference must resolve to a registered asset
- **WHEN** a run is recorded referencing a model asset ID that does not exist
- **THEN** the system rejects the write

#### Scenario: Many runs share one model asset reference
- **WHEN** many runs reference the same registered model asset
- **THEN** each run stores only the reference, and the model's identity is
  recorded once in the registry
