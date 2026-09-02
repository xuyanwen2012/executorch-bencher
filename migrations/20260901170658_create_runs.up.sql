CREATE TABLE runs (
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

    -- Device state (immutable snapshot)
    device_serial TEXT NOT NULL,
    bsp_version TEXT NOT NULL,
    sumd_driver_version TEXT NOT NULL,
    device_uptime_seconds INTEGER NOT NULL CHECK (device_uptime_seconds >= 0),
    battery_charging INTEGER NOT NULL CHECK (battery_charging IN (0, 1)),
    initial_temperature_celsius REAL NOT NULL CHECK (
        initial_temperature_celsius BETWEEN -40 AND 150
    ),
    max_temperature_celsius REAL NOT NULL CHECK (
        max_temperature_celsius BETWEEN -40 AND 150
    ),
    thermal_throttling INTEGER NOT NULL CHECK (thermal_throttling IN (0, 1)),

    -- Performance configuration
    gpu_clock_mhz INTEGER NOT NULL DEFAULT 980 CHECK (gpu_clock_mhz > 0),
    mif_clock_mhz INTEGER NOT NULL DEFAULT 5333 CHECK (mif_clock_mhz > 0),
    int_clock_mhz INTEGER NOT NULL DEFAULT 934 CHECK (int_clock_mhz > 0),

    -- Build and workload identity
    git_commit_sha TEXT NOT NULL,
    git_dirty INTEGER NOT NULL CHECK (git_dirty IN (0, 1)),
    executable_sha256 TEXT NOT NULL CHECK (length(executable_sha256) = 64),
    model_sha256 TEXT NOT NULL CHECK (length(model_sha256) = 64),
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
    stdout_artifact_id TEXT REFERENCES artifacts (id),
    stderr_artifact_id TEXT REFERENCES artifacts (id),
    crash_artifact_id TEXT REFERENCES artifacts (id),
    error_summary TEXT
);

CREATE INDEX runs_started_at_idx ON runs (started_at);
CREATE INDEX runs_device_serial_idx ON runs (device_serial);
CREATE INDEX runs_git_commit_sha_idx ON runs (git_commit_sha);
CREATE INDEX runs_bsp_version_idx ON runs (bsp_version);
CREATE INDEX runs_sumd_driver_version_idx ON runs (sumd_driver_version);
CREATE INDEX runs_exit_status_idx ON runs (exit_status);
CREATE INDEX runs_correctness_result_idx ON runs (correctness_result);
CREATE INDEX runs_executable_sha256_idx ON runs (executable_sha256);
CREATE INDEX runs_model_sha256_idx ON runs (model_sha256);
CREATE INDEX runs_prompt_sha256_idx ON runs (prompt_sha256);
