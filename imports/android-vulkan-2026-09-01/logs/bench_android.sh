#!/bin/bash
# Usage: bench_android.sh <device_serial> <log_path>
set -uo pipefail
DEV="$1"
LOG="$2"
> "$LOG"

bench() {
  local tag="$1" model="$2"
  for i in 1 2 3; do
    echo "=== $tag rep$i ===" >> "$LOG"
    adb -s "$DEV" shell "cd /data/local/tmp/llama && ./llama_main --model_path $model --tokenizer_path tokenizer.model --prompt_file prompt_2048.txt --max_new_tokens=1" 2>/dev/null | grep PyTorchObserver >> "$LOG"
  done
}

bench "1B-8da4w" llama3_2-1b_vulkan_8da4w.pte
bench "1B-4w"    llama3_2-1b_vulkan_4w.pte
bench "3B-8da4w" llama3_2-3b_vulkan_8da4w.pte
bench "3B-4w"    llama3_2-3b_vulkan_4w.pte
bench "8B-8da4w" llama3_1-8b_vulkan_8da4w.pte
bench "8B-4w"    llama3_1-8b_vulkan_4w.pte

echo "=== BENCH ALL DONE ===" >> "$LOG"
