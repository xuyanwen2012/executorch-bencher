-- Reverts to the Android-only `runs` schema. Linux rows and rows without an
-- executable hash cannot be represented there and are dropped; every other
-- Android row is carried over.
CREATE TABLE runs_old (
    id TEXT PRIMARY KEY,
    started_at TEXT NOT NULL,
    finished_at TEXT,
    repetition INTEGER NOT NULL CHECK (repetition >= 0),
    command_args TEXT NOT NULL CHECK (json_valid(command_args)),
    command_line TEXT,
    input_parameters TEXT NOT NULL CHECK (json_valid(input_parameters)),
    env_vars TEXT NOT NULL CHECK (json_valid(env_vars)),
    env_allowlist_version TEXT NOT NULL,
    collector_version TEXT NOT NULL,
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
    gpu_clock_mhz INTEGER NOT NULL DEFAULT 980 CHECK (gpu_clock_mhz > 0),
    mif_clock_mhz INTEGER NOT NULL DEFAULT 5333 CHECK (mif_clock_mhz > 0),
    int_clock_mhz INTEGER NOT NULL DEFAULT 934 CHECK (int_clock_mhz > 0),
    git_commit_sha TEXT NOT NULL,
    git_dirty INTEGER NOT NULL CHECK (git_dirty IN (0, 1)),
    executable_sha256 TEXT NOT NULL CHECK (length(executable_sha256) = 64),
    model_asset_id TEXT NOT NULL REFERENCES model_assets (id),
    prompt_sha256 TEXT NOT NULL CHECK (length(prompt_sha256) = 64),
    input_token_count INTEGER NOT NULL CHECK (input_token_count >= 0),
    output_token_count INTEGER NOT NULL CHECK (output_token_count >= 0),
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
    git_branch TEXT,
    git_commit_timestamp TEXT,
    git_commit_subject TEXT
);

INSERT INTO runs_old (
    id, started_at, finished_at, repetition, command_args, command_line,
    input_parameters, env_vars, env_allowlist_version, collector_version,
    device_serial, bsp_version, sumd_driver_version, device_uptime_seconds,
    battery_charging, initial_temperature_celsius, max_temperature_celsius,
    thermal_throttling, gpu_clock_mhz, mif_clock_mhz, int_clock_mhz,
    git_commit_sha, git_dirty, executable_sha256, model_asset_id, prompt_sha256,
    input_token_count, output_token_count, prefill_tokens_per_sec,
    decode_tokens_per_sec, exit_status, correctness_result,
    input_artifact_id, output_artifact_id, output_preview,
    stdout_artifact_id, stderr_artifact_id, crash_artifact_id, error_summary,
    git_branch, git_commit_timestamp, git_commit_subject
)
SELECT
    id, started_at, finished_at, repetition, command_args, command_line,
    input_parameters, env_vars, env_allowlist_version, collector_version,
    device_serial, bsp_version, sumd_driver_version, device_uptime_seconds,
    battery_charging, initial_temperature_celsius, max_temperature_celsius,
    thermal_throttling, gpu_clock_mhz, mif_clock_mhz, int_clock_mhz,
    git_commit_sha, git_dirty, executable_sha256, model_asset_id, prompt_sha256,
    input_token_count, output_token_count, prefill_tokens_per_sec,
    decode_tokens_per_sec, exit_status, correctness_result,
    input_artifact_id, output_artifact_id, output_preview,
    stdout_artifact_id, stderr_artifact_id, crash_artifact_id, error_summary,
    git_branch, git_commit_timestamp, git_commit_subject
FROM runs
WHERE platform = 'android' AND executable_sha256 IS NOT NULL;

DROP TABLE runs;
ALTER TABLE runs_old RENAME TO runs;

CREATE INDEX runs_started_at_idx ON runs (started_at);
CREATE INDEX runs_device_serial_idx ON runs (device_serial);
CREATE INDEX runs_git_commit_sha_idx ON runs (git_commit_sha);
CREATE INDEX runs_bsp_version_idx ON runs (bsp_version);
CREATE INDEX runs_sumd_driver_version_idx ON runs (sumd_driver_version);
CREATE INDEX runs_exit_status_idx ON runs (exit_status);
CREATE INDEX runs_correctness_result_idx ON runs (correctness_result);
CREATE INDEX runs_executable_sha256_idx ON runs (executable_sha256);
CREATE INDEX runs_model_asset_id_idx ON runs (model_asset_id);
CREATE INDEX runs_prompt_sha256_idx ON runs (prompt_sha256);
