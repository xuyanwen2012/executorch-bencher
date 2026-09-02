#!/usr/bin/env python3
"""Reference collector: record one `llama_main` repetition over HTTP.

Standard library only. Reads a `PyTorchObserver {...}` line (from stdin or
a file), uploads the captured stdout as an artifact, resolves the model
asset by SHA-256 (registering it from a backend-readable path if asked),
posts the run, and prints the dashboard URL for it.

Example (Linux box, prefill-only):

    ./llama_main --model_path=/mnt/models/m.pte --tokenizer_path=t.model \
        --prompt_file=prompt.txt --max_new_tokens=1 2>/dev/null \
      | grep PyTorchObserver \
      | python3 examples/post_run.py \
          --backend http://127.0.0.1:3100 \
          --model /mnt/linux-share/models/llama-3.2-1b/exported/llama3_2-1b_vulkan_8da4w.pte \
          --prompt-file prompt.txt --repetition 0 \
          --argv "--model_path=/mnt/models/m.pte --tokenizer_path=t.model --prompt_file=prompt.txt --max_new_tokens=1" \
          --git-sha e4d02f41f7909e8ed5bf4a14ffc520d733453d9f --git-branch release/1.4 \
          --executable ./cmake-out/examples/models/llama/llama_main \
          --benchmark prefill-2048

Use --platform android --device-class external --serial <ro.serialno>
--device-model 'Pixel 7a' ... for a retail phone; see docs/collector.md for
which host fields each platform and device class takes.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform as py_platform
import shlex
import subprocess
import sys
import urllib.error
import urllib.request
import uuid
from datetime import datetime, timezone


def sha256_file(path: str) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def request(method: str, url: str, body: bytes | None = None, content_type: str | None = None):
    req = urllib.request.Request(url, data=body, method=method)
    if content_type:
        req.add_header("Content-Type", content_type)
    try:
        with urllib.request.urlopen(req, timeout=60) as resp:
            return resp.status, json.loads(resp.read() or b"null")
    except urllib.error.HTTPError as err:
        payload = err.read()
        try:
            return err.code, json.loads(payload)
        except json.JSONDecodeError:
            return err.code, {"error": {"code": "non_json", "message": payload.decode(errors="replace")}}


def epoch_ms(ms: int) -> str:
    return datetime.fromtimestamp(ms / 1000, tz=timezone.utc).isoformat().replace("+00:00", "Z")


def linux_host_facts() -> dict:
    """Best-effort host description for a Linux box; override with flags."""
    facts = {"host_kernel": py_platform.release()}
    try:
        with open("/etc/os-release") as f:
            for line in f:
                if line.startswith("PRETTY_NAME="):
                    facts["host_os"] = line.split("=", 1)[1].strip().strip('"')
    except OSError:
        pass
    try:
        with open("/proc/cpuinfo") as f:
            for line in f:
                if line.startswith("model name"):
                    facts["host_cpu_model"] = line.split(":", 1)[1].strip()
                    break
        facts["host_cpu_count"] = os.cpu_count()
        with open("/proc/meminfo") as f:
            first = f.readline()
            facts["host_memory_bytes"] = int(first.split()[1]) * 1024
    except (OSError, ValueError, IndexError):
        pass
    try:
        out = subprocess.run(["vulkaninfo", "--summary"], capture_output=True, text=True, timeout=20).stdout
        for line in out.splitlines():
            if "deviceName" in line and "host_accelerator" not in facts:
                facts["host_accelerator"] = line.split("=", 1)[1].strip()
            if "driverInfo" in line and "host_accelerator_driver" not in facts:
                facts["host_accelerator_driver"] = line.split("=", 1)[1].strip()
    except (OSError, subprocess.SubprocessError):
        pass
    return facts


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--backend", default=os.environ.get("BENCH_BACKEND", "http://127.0.0.1:3100"))
    p.add_argument("--observer", help="file holding the PyTorchObserver line (default: stdin)")
    p.add_argument("--stdout-file", help="full captured stdout to upload (default: the observer line)")
    p.add_argument("--model", required=True, help="path of the .pte file (used for sha256; registered from this path if --register)")
    p.add_argument("--register", action="store_true", help="register the model from --model if unknown (path must be readable by the backend host)")
    p.add_argument("--prompt-file", help="prompt file (hashed and uploaded as the input artifact)")
    p.add_argument("--prompt-text", help="literal prompt (hashed and uploaded as the input artifact)")
    p.add_argument("--repetition", type=int, default=0)
    p.add_argument("--argv", required=True, help="the runner's argument string, e.g. '--model_path=... --max_new_tokens=1'")
    p.add_argument("--executable-name", default="llama_main")
    p.add_argument("--executable", help="path of the runner binary that ran, to hash; omit if it is not the exact binary")
    p.add_argument("--git-sha", required=True)
    p.add_argument("--git-dirty", action="store_true")
    p.add_argument("--git-branch")
    p.add_argument("--benchmark", default="prefill-2048", help="stored in input_parameters.benchmark")
    p.add_argument("--platform", choices=["linux", "android"], default="linux")
    p.add_argument("--device-class", choices=["external", "internal"], default="external")
    p.add_argument("--serial", help="device serial (android) or hostname (linux; default: this host)")
    p.add_argument("--device-model")
    for name in ["host-os", "host-kernel", "host-cpu-model", "host-accelerator", "host-accelerator-driver"]:
        p.add_argument(f"--{name}")
    p.add_argument("--host-cpu-count", type=int)
    p.add_argument("--host-memory-bytes", type=int)
    p.add_argument("--exit-status", default="succeeded")
    p.add_argument("--error-summary")
    p.add_argument("--collector-version", default="post_run.py/0.1")
    args = p.parse_args()

    raw = open(args.observer).read() if args.observer else sys.stdin.read()
    observer_line = next((l for l in raw.splitlines() if l.startswith("PyTorchObserver ")), None)
    if observer_line is None and args.exit_status == "succeeded":
        print("no PyTorchObserver line found; pass --exit-status crashed to record a failure", file=sys.stderr)
        return 2
    observer = json.loads(observer_line[len("PyTorchObserver "):]) if observer_line else {}

    base = args.backend.rstrip("/")

    # 1. Artifacts.
    stdout_bytes = open(args.stdout_file, "rb").read() if args.stdout_file else (observer_line + "\n").encode() if observer_line else b""
    stdout_id = None
    if stdout_bytes:
        status, body = request("POST", f"{base}/api/v1/artifacts?kind=stdout&original_name=rep{args.repetition}.stdout.txt", stdout_bytes, "text/plain")
        if status != 201:
            print("stdout upload failed:", body, file=sys.stderr)
            return 1
        stdout_id = body["id"]
    if args.prompt_file:
        prompt_bytes = open(args.prompt_file, "rb").read()
    elif args.prompt_text is not None:
        prompt_bytes = args.prompt_text.encode()
    else:
        print("give --prompt-file or --prompt-text", file=sys.stderr)
        return 2
    status, body = request("POST", f"{base}/api/v1/artifacts?kind=prompt&original_name=prompt.txt", prompt_bytes, "text/plain")
    if status != 201:
        print("prompt upload failed:", body, file=sys.stderr)
        return 1
    prompt_id, prompt_sha = body["id"], body["sha256"]

    # 2. Model asset by hash.
    model_sha = sha256_file(args.model)
    status, assets = request("GET", f"{base}/api/v1/models?sha256={model_sha}")
    if status != 200:
        print("model lookup failed:", assets, file=sys.stderr)
        return 1
    if assets:
        model_asset_id = assets[0]["id"]
    elif args.register:
        status, asset = request("POST", f"{base}/api/v1/models/register", json.dumps({"path": os.path.abspath(args.model)}).encode(), "application/json")
        if status != 201:
            print("model registration failed:", asset, file=sys.stderr)
            return 1
        model_asset_id = asset["id"]
    else:
        print(f"model {model_sha} is not registered; pass --register (path must be readable by the backend host)", file=sys.stderr)
        return 1

    # 3. The run.
    host = linux_host_facts() if args.platform == "linux" else {}
    for key in ["host_os", "host_kernel", "host_cpu_model", "host_cpu_count", "host_memory_bytes", "host_accelerator", "host_accelerator_driver"]:
        value = getattr(args, key)
        if value is not None:
            host[key] = value
    generated = observer.get("generated_tokens", 0)
    run = {
        "id": str(uuid.uuid4()),
        "started_at": epoch_ms(observer["model_load_start_ms"]) if observer else datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "finished_at": epoch_ms(observer["inference_end_ms"]) if observer else None,
        "repetition": args.repetition,
        "command_args": shlex.split(args.argv),
        "command_line": f"{args.executable_name} {args.argv}",
        "input_parameters": {"benchmark": args.benchmark, "observer": observer or None},
        "env_vars": {},
        "env_allowlist_version": "none",
        "collector_version": args.collector_version,
        "platform": args.platform,
        "device_class": args.device_class,
        "device_serial": args.serial or py_platform.node(),
        "device_model": args.device_model,
        "git_commit_sha": args.git_sha,
        "git_dirty": args.git_dirty,
        "git_branch": args.git_branch,
        "executable_sha256": sha256_file(args.executable) if args.executable else None,
        "model_asset_id": model_asset_id,
        "prompt_sha256": prompt_sha,
        "input_token_count": observer.get("prompt_tokens", 0),
        "output_token_count": generated,
        "prefill_tokens_per_sec": observer.get("prefill_token_per_sec", 0.0),
        "decode_tokens_per_sec": observer.get("decode_token_per_sec") if generated > 0 else None,
        "exit_status": args.exit_status,
        "correctness_result": "not_checked",
        "input_artifact_id": prompt_id,
        "stdout_artifact_id": stdout_id,
        "error_summary": args.error_summary,
        **host,
    }
    status, body = request("POST", f"{base}/api/v1/runs", json.dumps(run).encode(), "application/json")
    if status == 409:
        print("run already recorded (conflict); treating as success")
    elif status != 201:
        print("run rejected:", json.dumps(body, indent=2), file=sys.stderr)
        return 1
    print(f"recorded run {run['id']}: {base}/runs/{run['id']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
