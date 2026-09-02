#!/bin/bash
set -uo pipefail
cd /home/doremy/sarc-acl/executorch
RUNNER=./cmake-out/examples/models/llama/llama_main
PROMPT_FILE=/tmp/claude-1000/-home-doremy-sarc-acl-executorch/5795bc45-62c9-4c36-b02d-0fc01f53ed2c/scratchpad/prompt_2048.txt
LOG=/tmp/claude-1000/-home-doremy-sarc-acl-executorch/5795bc45-62c9-4c36-b02d-0fc01f53ed2c/scratchpad/bench_prefill_2048.log
> "$LOG"

bench() {
  local tag="$1" model="$2" tokenizer="$3"
  for i in 1 2 3; do
    echo "=== $tag rep$i ===" >> "$LOG"
    "$RUNNER" \
      --model_path="$model" \
      --tokenizer_path="$tokenizer" \
      --prompt_file="$PROMPT_FILE" \
      --max_new_tokens=1 2>/dev/null | grep PyTorchObserver >> "$LOG"
  done
}

bench "1B-8da4w" /mnt/linux-share/models/llama-3.2-1b/exported/llama3_2-1b_vulkan_8da4w.pte /mnt/linux-share/models/llama-3.2-1b/original/tokenizer.model
bench "1B-4w"    /mnt/linux-share/models/llama-3.2-1b/exported/llama3_2-1b_vulkan_4w.pte    /mnt/linux-share/models/llama-3.2-1b/original/tokenizer.model
bench "3B-8da4w" /mnt/linux-share/models/llama-3.2-3b/exported/llama3_2-3b_vulkan_8da4w.pte /mnt/linux-share/models/llama-3.2-3b/original/tokenizer.model
bench "3B-4w"    /mnt/linux-share/models/llama-3.2-3b/exported/llama3_2-3b_vulkan_4w.pte    /mnt/linux-share/models/llama-3.2-3b/original/tokenizer.model
bench "8B-8da4w" /mnt/linux-share/models/llama-3.1-8b/exported/llama3_1-8b_vulkan_8da4w.pte /mnt/linux-share/models/llama-3.1-8b/original/tokenizer.model
bench "8B-4w"    /mnt/linux-share/models/llama-3.1-8b/exported/llama3_1-8b_vulkan_4w.pte    /mnt/linux-share/models/llama-3.1-8b/original/tokenizer.model

echo "=== BENCH ALL DONE ===" >> "$LOG"
