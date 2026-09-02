# Writing a collector

A collector is whatever runs a benchmark and records it: today a Python
script driving `llama_main` on a Linux box or over `adb` on a phone. It
talks to the backend over plain HTTP with no authentication; everything
below is in the generated OpenAPI document (`GET /openapi.json`, Swagger
UI at `/docs`), and `examples/post_run.py` is a complete, dependency-free
reference implementation.

## Order of operations, per repetition

1. **Upload artifacts** you captured: stdout, stderr, a crash log, logcat,
   the prompt file, the generated output.

   ```
   POST /api/v1/artifacts?kind=stdout&original_name=rep1.stdout.txt
   Content-Type: text/plain
   <raw bytes>
   → 201 { "id": "...", "sha256": "...", "size_bytes": 120, "compression": "zstd" }
   ```

   `kind` is one of `prompt`, `stdout`, `stderr`, `output`, `crash_log`,
   `logcat`, `correctness_report`. Uploads are deduplicated by content, so
   re-uploading the same prompt returns the same id. The body is the raw
   bytes, not a multipart form.

2. **Resolve the model asset.** Hash the `.pte` file once (`sha256sum`) and
   look it up:

   ```
   GET /api/v1/models?sha256=<64 lowercase hex>
   → 200 [ { "id": "...", ... } ]      (empty list if unknown)
   ```

   If it is not registered, register it from a path the *backend host* can
   read (the NFS mount, not the phone). The path must be an absolute `.pte`
   file beneath one of the backend's registrable roots
   (`MODEL_REGISTER_ROOTS`; the real profile lists the model share) -
   anything else is a `400` naming `path`:

   ```
   POST /api/v1/models/register   { "path": "/mnt/linux-share/models/.../model.pte" }
   → 201 { "id": "...", "sha256": "...", ... }
   ```

3. **Post the run** once the process has exited, with everything you know:

   ```
   POST /api/v1/runs
   Content-Type: application/json
   { ...CreateRunRequest... }
   → 201 RunResponse   (what GET /api/v1/runs/{id} returns)
   ```

   The request has the same field names, units, and enumerations as the
   run response, so one shape serves both directions. Generate the run
   `id` yourself (UUID; v7 sorts by time).

## What to put in the request

A complete Linux submission (validated against the checked-in
`openapi/openapi.json`; `git_dirty` is true here because the checkout
carried a local compile fix):

```json
{
  "id": "01a0600a-1b2c-7d3e-8f40-5a6b7c8d9e0f",
  "started_at": "2026-09-01T23:11:07.131Z",
  "finished_at": "2026-09-01T23:11:08.001Z",
  "repetition": 0,
  "command_args": [
    "--model_path=/mnt/linux-share/models/llama-3.2-1b/exported/llama3_2-1b_vulkan_8da4w.pte",
    "--tokenizer_path=/mnt/linux-share/models/llama-3.2-1b/original/tokenizer.model",
    "--prompt_file=/tmp/prompt_2048.txt",
    "--max_new_tokens=1"
  ],
  "command_line": "llama_main --model_path=... --tokenizer_path=... --prompt_file=/tmp/prompt_2048.txt --max_new_tokens=1",
  "input_parameters": {
    "benchmark": "prefill-2048",
    "backend": "vulkan",
    "observer": {
      "prefill_token_per_sec": 5417.99,
      "prompt_tokens": 2048,
      "generated_tokens": 0
    }
  },
  "env_vars": {},
  "env_allowlist_version": "none",
  "collector_version": "my-bench.py/0.1",
  "platform": "linux",
  "device_class": "external",
  "device_serial": "ubuntu-lts",
  "device_model": null,
  "host_os": "Ubuntu 26.04.1 LTS",
  "host_kernel": "7.0.0-30-generic",
  "host_cpu_model": "Intel(R) Core(TM) i9-14900K",
  "host_cpu_count": 32,
  "host_memory_bytes": 67082768384,
  "host_accelerator": "NVIDIA GeForce RTX 4070 Ti SUPER",
  "host_accelerator_driver": "595.84",
  "git_commit_sha": "e4d02f41f7909e8ed5bf4a14ffc520d733453d9f",
  "git_dirty": true,
  "git_branch": "release/1.4",
  "git_commit_timestamp": "2026-08-14T18:12:28Z",
  "git_commit_subject": "[release/1.4] Fix CUDA shared library packaging (#21850)",
  "executable_sha256": null,
  "model_asset_id": "01a05f4c-2d1e-7a0b-9c3d-4e5f60718293",
  "prompt_sha256": "61a7273e2b8eb92ce7a53db4d6df43802a8412d55b6b1b56dbe9a11604c26104",
  "input_token_count": 2048,
  "output_token_count": 0,
  "prefill_tokens_per_sec": 5417.99,
  "decode_tokens_per_sec": null,
  "exit_status": "succeeded",
  "correctness_result": "not_checked",
  "input_artifact_id": "01a05f4c-3e2f-7b1c-8d4e-5f6071829304",
  "stdout_artifact_id": "01a05f4c-4f30-7c2d-9e5f-607182930415",
  "output_preview": null,
  "error_summary": null
}
```


Identity and outcome, always:

| Field | Notes |
|---|---|
| `id`, `started_at`, `finished_at` | RFC 3339 UTC. `finished_at` is null if the process never finished. |
| `repetition` | zero-based |
| `command_args`, `command_line` | exact argv array; human-readable line |
| `input_parameters` | any JSON object: benchmark name, backend, export recipe, the raw `PyTorchObserver` line |
| `env_vars`, `env_allowlist_version` | only allowlisted variables; `{}` and `"none"` if you capture none |
| `collector_version` | your script's version string |
| `git_commit_sha`, `git_dirty`, `git_branch`, `git_commit_timestamp`, `git_commit_subject` | of the ExecuTorch checkout the runner was built from |
| `executable_sha256` | of the runner binary, or `null` if you cannot hash the binary that ran; never a guess |
| `model_asset_id`, `prompt_sha256`, `input_token_count`, `output_token_count` | |
| `prefill_tokens_per_sec`, `decode_tokens_per_sec` | decode `null` when nothing was generated (the runner prints `0` then; do not store it as a measurement) |
| `exit_status`, `correctness_result` | `succeeded` / `crashed` / `timed_out` / `cancelled` / `infrastructure_error`; `passed` / `failed` / `not_checked` / `validator_error` |
| `*_artifact_id`, `output_preview`, `error_summary` | optional |

Host, by platform and device class:

- **`platform: "linux"`**, `device_class: "external"`: `device_serial` is
  the hostname. Required: `host_os`, `host_kernel`, `host_cpu_model`,
  `host_accelerator` (the Vulkan device name). Optional: `host_cpu_count`,
  `host_memory_bytes`, `host_accelerator_driver`, `device_uptime_seconds`,
  `thermal_throttling`. Must be null: `bsp_version`, `sumd_driver_version`,
  the three clocks, `battery_charging`, temperatures.
- **`platform: "android"`, `device_class: "external"`** (a retail,
  unrooted phone): `device_serial` is `ro.serialno`. Everything is
  optional; record what `adb shell` can tell you: `device_model`
  (`ro.product.model`), `host_os` (Android release and build id),
  `host_kernel` (`uname -r`), `host_cpu_model` (`ro.soc.model`),
  `host_cpu_count`, `host_memory_bytes`, `host_accelerator` and
  `host_accelerator_driver` (from `cmd gpu vkjson`). `bsp_version`,
  `sumd_driver_version`, and the three clocks must be given all together
  or not at all.
- **`platform: "android"`, `device_class: "internal"`** (a lab phone under
  full control): additionally required, all of them: `bsp_version`,
  `sumd_driver_version`, `gpu_clock_mhz`, `mif_clock_mhz`, `int_clock_mhz`,
  `device_uptime_seconds`, `battery_charging`,
  `initial_temperature_celsius`, `max_temperature_celsius`,
  `thermal_throttling`.

A rejected request is `400` with `error.code = "invalid_request"` and
`error.details.field` naming the field. Nothing is stored on rejection.

## Retries

Network failures happen after the server may have committed. Because you
chose the `id`, just retry the same body: a second submission of an
existing id returns `409` with `error.code = "conflict"` and leaves the
stored run untouched. Treat `409` as success and, if you want to be sure,
`GET /api/v1/runs/{id}`.

## Live updates

`GET /api/v1/events` is a Server-Sent Events stream: `run.created`,
`artifact.created`, `model.registered`, each with a JSON payload, plus
keep-alive comments. It exists so the dashboard can refresh while your
session runs; a collector does not need it. Nothing is replayed after a
reconnect, so a consumer re-fetches the REST endpoints when it reconnects.

## Failed repetitions

Record them. A rep that crashed, rebooted the phone, or never ran because
the device was unreachable is a run with `exit_status` `crashed` or
`infrastructure_error`, `finished_at` null, `prefill_tokens_per_sec` 0,
`output_token_count` 0, and an `error_summary`. The results view counts it
as not succeeded and keeps it out of every statistic, which is exactly the
information a reader wants.
