// Verifies live refresh end to end: open the runs page, post a run through
// the backend, and expect the new run to appear without a reload. Run with
// the dev server on DASH (default http://127.0.0.1:3111) proxying to a
// backend, and a model registered in that backend.
//   bun scripts/live-check.ts <model.pte path readable by the backend>
import { chromium } from "playwright";

const DASH = process.env.DASH ?? "http://127.0.0.1:3111";
const model = process.argv[2];
if (!model) {
  console.error("usage: bun scripts/live-check.ts <model.pte>");
  process.exit(2);
}
const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
await page.goto(`${DASH}/runs`, { waitUntil: "networkidle" });
const indicator = page.getByTestId("live-indicator");
await indicator.waitFor();
const stateBefore = await indicator.getAttribute("data-state");
const rowsBefore = await page.locator("tbody tr").count();

const observer = `PyTorchObserver ${JSON.stringify({
  prefill_token_per_sec: 4321.5,
  prompt_tokens: 2048,
  generated_tokens: 0,
  model_load_start_ms: Date.now() - 3000,
  model_load_end_ms: Date.now() - 2500,
  inference_start_ms: Date.now() - 2500,
  inference_end_ms: Date.now() - 1000,
})}`;
const proc = Bun.spawn(
  [
    "python3", "../examples/post_run.py", "--backend", process.env.BACKEND ?? "http://127.0.0.1:3110",
    "--model", model, "--prompt-text", "live check", "--repetition", "0",
    "--argv", `--model_path=${model} --max_new_tokens=1`, "--git-sha", "3333333333333333333333333333333333333333",
    "--git-branch", "main", "--benchmark", "live-check", "--serial", "live-check-host",
    "--host-accelerator", "NVIDIA GeForce RTX 4070 Ti SUPER",
  ],
  { stdin: new Response(observer), stdout: "pipe", stderr: "pipe" },
);
const out = await new Response(proc.stdout).text();
const err = await new Response(proc.stderr).text();
if ((await proc.exited) !== 0) {
  console.error("post_run.py failed:", out, err);
  await browser.close();
  process.exit(1);
}
const runId = out.match(/recorded run ([0-9a-f-]{36})/)?.[1];
console.log("posted", runId);

// Wait up to 5 s for the row to appear without navigating.
let appeared = false;
for (let i = 0; i < 25 && !appeared; i++) {
  await page.waitForTimeout(200);
  appeared = (await page.locator(`a[href="/runs/${runId}"]`).count()) > 0;
}
const rowsAfter = await page.locator("tbody tr").count();
const stateAfter = await indicator.getAttribute("data-state");
console.log({ stateBefore, stateAfter, rowsBefore, rowsAfter, appeared, reloaded: false });
await browser.close();
if (!appeared || stateAfter !== "live") process.exit(1);
console.log("live refresh verified");
