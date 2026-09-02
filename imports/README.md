# Imported benchmark results

Raw result files from benchmark sessions that predate a collector, plus one
import manifest per log. `cargo run --bin import-observer-log -- <manifest>`
(or `just import-log <manifest>` / `just import-all`) turns each log into
runs in the **real** database. Imports are idempotent: a run is identified
by its log file's SHA-256, tag, and repetition, and is skipped if present.

Each manifest carries everything the log itself does not: host identity and
hardware, git provenance (including which files were locally patched),
the runner binary's hash when it was preserved, prompt and model
identities, and the command template. Values that were not captured are
recorded as null, never guessed, with the reason in the manifest.

| Directory | What | Runs |
|---|---|---|
| `linux-vulkan-2026-09-01/` | ExecuTorch `llama_main`, Vulkan backend, Llama 3.2 1B/3B and 3.1 8B at `8da4w` and `4w`, on three Linux boxes (RTX 4070 Ti SUPER, Radeon 780M, Arc B580). Prefill-only at 2048 tokens (3 reps per config per host) plus one decode benchmark on the RTX host. | 54 prefill + 18 decode |
| `android-vulkan-2026-09-01/` | Same runner (cross-compiled arm64-v8a, Vulkan) and same six models on two retail, unrooted phones: Pixel 7a (Tensor G2 / Mali-G710) and Galaxy S24 (Exynos 2400 / Xclipse 940). Prefill-only at 2048 tokens, 3 reps per config; both 8B configurations rebooted the Pixel and are recorded as failed runs. Plus one decode smoke line per phone. | 36 prefill (30 succeeded) + 2 smoke |
