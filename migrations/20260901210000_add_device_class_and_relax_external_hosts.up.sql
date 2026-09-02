-- Distinguish internal (lab) devices from external ones and relax the
-- Android snapshot for external devices. See `specs/benchmark-schema` -
-- "Host state is captured as a platform-specific immutable snapshot".
--
-- Everything measured so far ran on external hosts: retail, unrooted
-- Android phones (no SUMD, no BSP control, no clock pinning) and Linux
-- boxes. Their numbers matter, so the rigorous Android snapshot (BSP, SUMD,
-- pinned clocks, battery, temperatures, throttling) is now required only
-- on `internal` Android devices; external Android rows record what the
-- phone can report and leave the rest null. The `host_*` description
-- columns become usable on Android too (build, kernel, SoC, GPU, driver),
-- and every run gains an optional `device_model`. Rebuilt because SQLite
-- cannot change a CHECK constraint in place. Existing Android rows all
-- carry the full lab snapshot and are classified `internal`; Linux rows
-- are `external`.
CREATE TABLE runs_new (
    id TEXT PRIMARY KEY,

    -- Run metadata
    started_at TEXT NOT NULL,
    finished_at TEXT,
    repetition INTEGER NOT NULL CHECK (repetition >= 0),
    command_args TEXT NOT NULL CHECK (json_valid(command_args)),
    command_line TEXT,
    input_parameters TEXT NOT NULL CHECK (json_valid(input_parameters)),
    env_vars TEXT NOT NULL CHECK (json_valid(env_vars)),
    env_allowlist_version TEXT NOT NULL,
    collector_version TEXT NOT NULL,

    -- Host identity (every platform)
    platform TEXT NOT NULL CHECK (platform IN ('android', 'linux')),
    -- `internal`: a lab device under full control (rooted development
    -- phone with SUMD/BSP/clock pinning); its rigorous snapshot is
    -- required. `external`: a retail phone or any Linux box; whatever it
    -- can report is recorded, the rest stays null.
    device_class TEXT NOT NULL CHECK (device_class IN ('internal', 'external')),
    -- Android: the device serial. Linux: the hostname.
    device_serial TEXT NOT NULL,
    -- Product/model name as the host reports it (e.g. `SM-S926B`).
    device_model TEXT,
    device_uptime_seconds INTEGER CHECK (
        device_uptime_seconds IS NULL OR device_uptime_seconds >= 0
    ),
    thermal_throttling INTEGER CHECK (
        thermal_throttling IS NULL OR thermal_throttling IN (0, 1)
    ),

    -- Android lab snapshot (required on internal android rows; optional on
    -- external android rows; null on linux)
    bsp_version TEXT,
    sumd_driver_version TEXT,
    battery_charging INTEGER CHECK (
        battery_charging IS NULL OR battery_charging IN (0, 1)
    ),
    initial_temperature_celsius REAL CHECK (
        initial_temperature_celsius IS NULL
        OR initial_temperature_celsius BETWEEN -40 AND 150
    ),
    max_temperature_celsius REAL CHECK (
        max_temperature_celsius IS NULL
        OR max_temperature_celsius BETWEEN -40 AND 150
    ),

    -- Android pinned clocks (internal android rows; optional on external)
    gpu_clock_mhz INTEGER CHECK (gpu_clock_mhz IS NULL OR gpu_clock_mhz > 0),
    mif_clock_mhz INTEGER CHECK (mif_clock_mhz IS NULL OR mif_clock_mhz > 0),
    int_clock_mhz INTEGER CHECK (int_clock_mhz IS NULL OR int_clock_mhz > 0),

    -- Host description (required on linux; optional on android, where it
    -- holds the Android build, kernel, SoC, and GPU as the phone reports them)
    host_os TEXT,
    host_kernel TEXT,
    host_cpu_model TEXT,
    host_cpu_count INTEGER CHECK (host_cpu_count IS NULL OR host_cpu_count > 0),
    host_memory_bytes INTEGER CHECK (host_memory_bytes IS NULL OR host_memory_bytes >= 0),
    -- The accelerator the runtime backend executed on, as the backend
    -- reports it (e.g. the Vulkan device name), and its driver version.
    host_accelerator TEXT,
    host_accelerator_driver TEXT,

    -- Build and workload identity
    git_commit_sha TEXT NOT NULL,
    git_dirty INTEGER NOT NULL CHECK (git_dirty IN (0, 1)),
    git_branch TEXT,
    git_commit_timestamp TEXT,
    git_commit_subject TEXT,
    -- Null when the executable's identity was not preserved (e.g. runs
    -- imported from logs after the binary was rebuilt); never a guess.
    executable_sha256 TEXT CHECK (
        executable_sha256 IS NULL OR length(executable_sha256) = 64
    ),
    model_asset_id TEXT NOT NULL REFERENCES model_assets (id),
    prompt_sha256 TEXT NOT NULL CHECK (length(prompt_sha256) = 64),
    input_token_count INTEGER NOT NULL CHECK (input_token_count >= 0),
    output_token_count INTEGER NOT NULL CHECK (output_token_count >= 0),

    -- Results
    prefill_tokens_per_sec REAL NOT NULL CHECK (prefill_tokens_per_sec >= 0),
    decode_tokens_per_sec REAL CHECK (
        decode_tokens_per_sec IS NULL OR decode_tokens_per_sec >= 0
    ),
    exit_status TEXT NOT NULL CHECK (
        exit_status IN (
            'succeeded', 'crashed', 'timed_out', 'cancelled', 'infrastructure_error'
        )
    ),
    correctness_result TEXT NOT NULL CHECK (
        correctness_result IN ('passed', 'failed', 'not_checked', 'validator_error')
    ),
    input_artifact_id TEXT REFERENCES artifacts (id),
    output_artifact_id TEXT REFERENCES artifacts (id),
    output_preview TEXT,
    stdout_artifact_id TEXT REFERENCES artifacts (id),
    stderr_artifact_id TEXT REFERENCES artifacts (id),
    crash_artifact_id TEXT REFERENCES artifacts (id),
    error_summary TEXT,

    -- Each platform and device class carries exactly its own snapshot.
    CHECK (
        (
            platform = 'linux'
            AND host_os IS NOT NULL
            AND host_kernel IS NOT NULL
            AND host_cpu_model IS NOT NULL
            AND host_accelerator IS NOT NULL
            AND bsp_version IS NULL
            AND sumd_driver_version IS NULL
            AND battery_charging IS NULL
            AND initial_temperature_celsius IS NULL
            AND max_temperature_celsius IS NULL
            AND gpu_clock_mhz IS NULL
            AND mif_clock_mhz IS NULL
            AND int_clock_mhz IS NULL
        )
        OR (
            platform = 'android'
            AND device_class = 'internal'
            AND bsp_version IS NOT NULL
            AND sumd_driver_version IS NOT NULL
            AND device_uptime_seconds IS NOT NULL
            AND battery_charging IS NOT NULL
            AND initial_temperature_celsius IS NOT NULL
            AND max_temperature_celsius IS NOT NULL
            AND thermal_throttling IS NOT NULL
            AND gpu_clock_mhz IS NOT NULL
            AND mif_clock_mhz IS NOT NULL
            AND int_clock_mhz IS NOT NULL
        )
        OR (
            platform = 'android'
            AND device_class = 'external'
            -- BSP, SUMD, and pinned clocks exist only on lab-controlled
            -- devices: a retail phone carries all five or none.
            AND (
                (
                    bsp_version IS NULL AND sumd_driver_version IS NULL
                    AND gpu_clock_mhz IS NULL AND mif_clock_mhz IS NULL AND int_clock_mhz IS NULL
                )
                OR (
                    bsp_version IS NOT NULL AND sumd_driver_version IS NOT NULL
                    AND gpu_clock_mhz IS NOT NULL AND mif_clock_mhz IS NOT NULL AND int_clock_mhz IS NOT NULL
                )
            )
        )
    )
);

INSERT INTO runs_new (
    id, started_at, finished_at, repetition, command_args, command_line,
    input_parameters, env_vars, env_allowlist_version, collector_version,
    platform, device_serial, device_uptime_seconds, thermal_throttling,
    bsp_version, sumd_driver_version, battery_charging,
    initial_temperature_celsius, max_temperature_celsius,
    gpu_clock_mhz, mif_clock_mhz, int_clock_mhz,
    host_os, host_kernel, host_cpu_model, host_cpu_count, host_memory_bytes,
    host_accelerator, host_accelerator_driver,
    git_commit_sha, git_dirty, git_branch, git_commit_timestamp, git_commit_subject,
    executable_sha256, model_asset_id, prompt_sha256,
    input_token_count, output_token_count, prefill_tokens_per_sec,
    decode_tokens_per_sec, exit_status, correctness_result,
    input_artifact_id, output_artifact_id, output_preview,
    stdout_artifact_id, stderr_artifact_id, crash_artifact_id, error_summary,
    device_class, device_model
)
SELECT
    id, started_at, finished_at, repetition, command_args, command_line,
    input_parameters, env_vars, env_allowlist_version, collector_version,
    platform, device_serial, device_uptime_seconds, thermal_throttling,
    bsp_version, sumd_driver_version, battery_charging,
    initial_temperature_celsius, max_temperature_celsius,
    gpu_clock_mhz, mif_clock_mhz, int_clock_mhz,
    host_os, host_kernel, host_cpu_model, host_cpu_count, host_memory_bytes,
    host_accelerator, host_accelerator_driver,
    git_commit_sha, git_dirty, git_branch, git_commit_timestamp, git_commit_subject,
    executable_sha256, model_asset_id, prompt_sha256,
    input_token_count, output_token_count, prefill_tokens_per_sec,
    decode_tokens_per_sec, exit_status, correctness_result,
    input_artifact_id, output_artifact_id, output_preview,
    stdout_artifact_id, stderr_artifact_id, crash_artifact_id, error_summary,
    CASE WHEN platform = 'android' THEN 'internal' ELSE 'external' END,
    NULL
FROM runs;

DROP TABLE runs;
ALTER TABLE runs_new RENAME TO runs;

CREATE INDEX runs_started_at_idx ON runs (started_at);
CREATE INDEX runs_platform_idx ON runs (platform);
CREATE INDEX runs_device_class_idx ON runs (device_class);
CREATE INDEX runs_device_serial_idx ON runs (device_serial);
CREATE INDEX runs_git_commit_sha_idx ON runs (git_commit_sha);
CREATE INDEX runs_bsp_version_idx ON runs (bsp_version);
CREATE INDEX runs_sumd_driver_version_idx ON runs (sumd_driver_version);
CREATE INDEX runs_host_accelerator_idx ON runs (host_accelerator);
CREATE INDEX runs_exit_status_idx ON runs (exit_status);
CREATE INDEX runs_correctness_result_idx ON runs (correctness_result);
CREATE INDEX runs_executable_sha256_idx ON runs (executable_sha256);
CREATE INDEX runs_model_asset_id_idx ON runs (model_asset_id);
CREATE INDEX runs_prompt_sha256_idx ON runs (prompt_sha256);
