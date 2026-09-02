#!/usr/bin/env python3
"""Deterministic fake benchmark used to exercise collectors and the backend."""

from __future__ import annotations

import argparse
import json
import sys
import time
import zipfile
from pathlib import Path


def numbers(paths: list[Path]) -> list[float]:
    values: list[float] = []
    for path in paths:
        for token in path.read_text(encoding="utf-8").split():
            values.append(float(token))
    return values


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", action="append", required=True, type=Path)
    parser.add_argument("--scale", type=float, default=1.0)
    parser.add_argument("--bias", type=float, default=0.0)
    parser.add_argument("--mode", choices=("success", "incorrect", "crash"), default="success")
    parser.add_argument("--artifact-dir", required=True, type=Path)
    parser.add_argument("--result-json", required=True, type=Path)
    args = parser.parse_args()

    started_ns = time.time_ns()
    args.artifact_dir.mkdir(parents=True, exist_ok=True)
    args.result_json.parent.mkdir(parents=True, exist_ok=True)
    values = numbers(args.input)
    expected = sum(values) * args.scale + args.bias
    answer = expected + 1.0 if args.mode == "incorrect" else expected

    (args.artifact_dir / "answer.txt").write_text(f"{answer}\n", encoding="utf-8")
    (args.artifact_dir / "counter-000.json").write_text(
        json.dumps({"name": "fake_cycles", "value": len(values) * 1000}) + "\n", encoding="utf-8"
    )
    (args.artifact_dir / "counter-001.json").write_text(
        json.dumps({"name": "fake_cache_misses", "value": len(values) * 7}) + "\n", encoding="utf-8"
    )
    (args.artifact_dir / "parameters.json").write_text(
        json.dumps({"inputs": [str(p) for p in args.input], "scale": args.scale, "bias": args.bias}, indent=2) + "\n",
        encoding="utf-8",
    )

    correctness = "passed" if args.mode == "success" else "failed" if args.mode == "incorrect" else "not_checked"
    error_summary = "mock experiment crashed after producing counters" if args.mode == "crash" else None
    crash_log = args.artifact_dir / "crash.log"
    if args.mode == "crash":
        crash_log.write_text(
            "mock fatal error: simulated accelerator reset\n", encoding="utf-8"
        )

    bundle = args.result_json.with_suffix(".artifacts.zip")
    with zipfile.ZipFile(bundle, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        produced = ["answer.txt", "counter-000.json", "counter-001.json", "parameters.json"]
        if args.mode == "crash":
            produced.append("crash.log")
        for name in produced:
            path = args.artifact_dir / name
            archive.write(path, arcname=path.name)

    elapsed_ms = max(1, (time.time_ns() - started_ns) // 1_000_000)
    result = {
        "schema_version": 1,
        "mode": args.mode,
        "answer": answer,
        "expected_answer": expected,
        "correctness_result": correctness,
        "exit_status": "crashed" if args.mode == "crash" else "succeeded",
        "error_summary": error_summary,
        "input_value_count": len(values),
        "elapsed_ms": elapsed_ms,
        "artifact_bundle": str(bundle),
        "crash_log": str(crash_log) if args.mode == "crash" else None,
    }
    args.result_json.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")

    print(json.dumps({"event": "mock_experiment.finished", **result}))
    if args.mode == "crash":
        print("simulated accelerator reset", file=sys.stderr)
        return 17
    print(f"answer={answer}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
