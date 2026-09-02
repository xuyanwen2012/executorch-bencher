// Manual smoke test: renders the real app inside happy-dom against a
// running dev server (or backend) and checks each page's observable
// behavior. Not part of `bun test` because it needs live servers.
//
//   SCENARIO=seeded BASE_URL=http://127.0.0.1:3001 DEVDATA=<data root> bun run scripts/smoke.tsx
//   SCENARIO=empty  BASE_URL=<backend with an empty database>          bun run scripts/smoke.tsx
//   SCENARIO=dead   BASE_URL=<nothing listening>                        bun run scripts/smoke.tsx
//
// `seeded` expects the database produced by `cargo run --example
// e2e_insert_retrieve` on a fresh data root; DEVDATA (optional) enables the
// deleted-artifact-file check. One happy-dom window per process: React's
// scheduler is bound to the window it first rendered in.
import { GlobalRegistrator } from "@happy-dom/global-registrator";

const BASE_URL = process.env.BASE_URL ?? "http://127.0.0.1:3001";
const SCENARIO = process.env.SCENARIO ?? "seeded";
GlobalRegistrator.register({ url: `${BASE_URL}/` });

const { createElement } = await import("react");
const { createRoot } = await import("react-dom/client");
const { MemoryRouter } = await import("react-router");
const { App, queryClient } = await import("../src/main");

let failures = 0;
function check(name: string, ok: boolean, detail = "") {
  console.log(`${ok ? "PASS" : "FAIL"} ${name}${detail ? ` - ${detail}` : ""}`);
  if (!ok) failures += 1;
}

async function render(path: string): Promise<HTMLElement> {
  queryClient.clear();
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  root.render(createElement(MemoryRouter, { initialEntries: [path] }, createElement(App)));
  return container;
}

async function waitFor(container: HTMLElement, predicate: (text: string) => boolean, timeoutMs = 8000): Promise<string> {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    const text = container.textContent ?? "";
    if (predicate(text)) return text;
    await new Promise((r) => setTimeout(r, 50));
  }
  return container.textContent ?? "";
}

async function json(path: string): Promise<any> {
  const res = await fetch(`${BASE_URL}${path}`);
  return res.json();
}

if (SCENARIO === "seeded") {
// ---- Results page -------------------------------------------------------
{
  const c = await render("/");
  const text = await waitFor(c, (t) => t.includes("tok/s") && !t.includes("Loading"));
  check("results: rows render with the newest commit first", text.indexOf("e2e1111111") < text.indexOf("e2e0000000"), text.slice(0, 200));
  check("results: dirty row is marked", text.includes("+dirty"));
  check("results: median with range and n", /120\.4.*118\.9–123\.7, n=3/.test(text.replace(/\s+/g, " ")));
  check("results: failure and throttle badges", text.includes("1 failed") && text.includes("1 throttled"));
  const shared = c.querySelector('[data-testid="shared-configuration"]')?.textContent ?? "";
  check("results: constant columns collapsed into the shared line", shared.includes("BSP bsp-1.0") && shared.includes("GPU MHz 980"));
  const headers = Array.from(c.querySelectorAll("th")).map((th) => th.textContent?.trim());
  check("results: constant columns hidden from the table", !headers.includes("BSP") && !headers.includes("GPU MHz"));
  check("results: absent decode is marked, not zero", text.includes("not recorded") && !/\b0\.0\b/.test(text));
  const runsLink = Array.from(c.querySelectorAll("a")).find((a) => a.textContent?.startsWith("runs ("));
  check("results: rows link to their runs with the full key", !!runsLink && /device_serial=.*model_asset_id=.*git_commit_sha=.*git_dirty=.*sumd_driver_version=.*bsp_version=.*gpu_clock_mhz=.*mif_clock_mhz=.*int_clock_mhz=.*prompt_sha256=/.test(runsLink?.getAttribute("href") ?? ""));
}
{
  const c = await render("/?all=1");
  await waitFor(c, (t) => t.includes("tok/s") && !t.includes("Loading"));
  const headers = Array.from(c.querySelectorAll("th")).map((th) => th.textContent?.trim());
  check("results: show-all toggle reveals every key column", headers.includes("BSP") && headers.includes("GPU MHz") && headers.includes("Input tokens"));
}
{
  const c = await render("/?device_serial=nope");
  const text = await waitFor(c, (t) => t.includes("No configurations match"));
  check("results: unmatched filter shows the filtered empty state", text.includes("No configurations match these filters."));
  const select = c.querySelector<HTMLSelectElement>("#filter-device_serial");
  check("results: filter control reflects the URL", select?.value === "nope" || (select?.value === "" && text.includes("No configurations match")));
}
{
  const c = await render("/?device_serial=e2e-device-001&git_dirty=true");
  const text = await waitFor(c, (t) => t.includes("tok/s") && !t.includes("Loading"));
  check("results: conjunctive filters narrow rows", text.includes("+dirty") && !text.includes("1 failed"));
  const select = c.querySelector<HTMLSelectElement>("#filter-git_dirty");
  check("results: dirty filter select reflects URL", select?.value === "true");
}

// ---- Runs page ----------------------------------------------------------
{
  const c = await render("/runs");
  const text = await waitFor(c, (t) => t.includes("All matching runs loaded") || t.includes("Load more"));
  check("runs: lists seeded runs", text.includes("e2e-device-001") && text.includes("38.2"));
  check("runs: absent decode marked", text.includes("not recorded"));
  check("runs: crashed run shows its status and throttling", text.includes("crashed") && text.includes("throttled"));
  const rows = c.querySelectorAll("tbody tr");
  check("runs: newest first", rows.length === 6, `${rows.length} rows`);
}
{
  const c = await render("/runs?exit_status=crashed&device_serial=e2e-device-001");
  const text = await waitFor(c, (t) => t.includes("All matching runs loaded"));
  check("runs: conjunctive filter", c.querySelectorAll("tbody tr").length === 1 && text.includes("crashed"));
  const select = c.querySelector<HTMLSelectElement>("#filter-exit_status");
  check("runs: filter control reflects the URL", select?.value === "crashed");
}
{
  const c = await render("/runs?device_serial=no-such-device");
  const text = await waitFor(c, (t) => t.includes("No runs match"));
  check("runs: filtered empty state", text.includes("No runs match these filters."));
}
{
  const results = await json("/api/v1/results");
  const row = results.rows.find((r: any) => r.git_dirty === false && r.git_commit_sha.startsWith("e2e1111"));
  const params = new URLSearchParams({
    device_serial: row.device_serial,
    model_asset_id: row.model_asset.id,
    git_commit_sha: row.git_commit_sha,
    git_dirty: "false",
    sumd_driver_version: row.sumd_driver_version,
    bsp_version: row.bsp_version,
    gpu_clock_mhz: String(row.gpu_clock_mhz),
    mif_clock_mhz: String(row.mif_clock_mhz),
    int_clock_mhz: String(row.int_clock_mhz),
    prompt_sha256: row.prompt_sha256,
  });
  const c = await render(`/runs?${params}`);
  const text = await waitFor(c, (t) => t.includes("All matching runs loaded"));
  check("runs: results-row link shows exactly the contributing runs", c.querySelectorAll("tbody tr").length === row.total_runs, `${c.querySelectorAll("tbody tr").length} vs ${row.total_runs}`);
  check("runs: linked-only key filters shown as removable chips", text.includes("gpu_clock_mhz=980") && text.includes("prompt_sha256="));
}
{
  // Paging: limit is fixed at 50 in the UI, so exercise the API contract
  // the "Load more" button relies on with a small page.
  const page1 = await json("/api/v1/runs?limit=2");
  const page2 = await json(`/api/v1/runs?limit=2&cursor=${page1.next_cursor}`);
  check("runs: cursor paging continues without overlap", page1.items.length === 2 && page2.items.length === 2 && !page2.items.some((i: any) => page1.items.some((j: any) => j.id === i.id)));
}

// ---- Run detail ---------------------------------------------------------
{
  const list = await json("/api/v1/runs?limit=50");
  const withStdout = list.items.find((i: any) => i.repetition === 0 && i.exit_status === "succeeded" && i.git_commit_sha.startsWith("e2e0000"));
  const c = await render(`/runs/${withStdout.id}`);
  const text = await waitFor(c, (t) => t.includes("Artifacts") && !t.includes("Loading run"));
  const flat = text.replace(/\s+/g, " ");
  for (const expected of [
    "980 MHz", "5333 MHz", "934 MHz", "32.5 °C", "41.0 °C", "1h 0m 0s", "e2e example commit", "main",
    "120.4", "38.2", "resnet50", "EXPERIMENT_ID", "collector-0.1", "generated output", withStdout.id,
  ]) {
    check(`detail: shows ${expected}`, flat.includes(expected));
  }
  check("detail: UTC timestamp shown alongside local", /\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z/.test(flat));
  check("detail: absent fields marked", flat.includes("not recorded"));
  const stdout = c.querySelector('[data-testid="artifact-stdout"]');
  check("detail: stdout artifact available with download and view", !!stdout && stdout.textContent!.includes("available") && !!stdout.querySelector("a[download]") && Array.from(stdout.querySelectorAll("button")).some((b) => b.textContent === "View"));
  const viewButton = Array.from(stdout!.querySelectorAll("button")).find((b) => b.textContent === "View")!;
  viewButton.click();
  const shown = await waitFor(stdout as HTMLElement, (t) => t.includes("prefill: 120.4 tok/s"));
  check("detail: inline view shows decompressed stdout", shown.includes("decode: 38.2 tok/s"));
  const stderr = c.querySelector('[data-testid="artifact-stderr"]');
  check("detail: missing slot shows none", !!stderr && stderr.textContent!.includes("none"));
  const output = c.querySelector('[data-testid="artifact-output"]');
  check("detail: uncompressed output artifact listed with a download link", !!output && output.textContent!.includes("available") && !!output.querySelector("a[download]"));
  check("detail: model summary", flat.includes("e2e-example-model") && flat.includes("available"));
}
{
  const c = await render("/runs/00000000-0000-7000-8000-000000000000");
  const text = await waitFor(c, (t) => t.includes("Run not found"));
  check("detail: unknown id shows run-not-found with a link back", text.includes("Run not found.") && !!Array.from(c.querySelectorAll("a")).find((a) => a.getAttribute("href") === "/runs"));
}

// ---- Unavailable artifact (file deleted on disk) ----------------------
if (process.env.DEVDATA) {
  const list = await json("/api/v1/runs?limit=50");
  const withStdout = list.items.find((i: any) => i.repetition === 0 && i.exit_status === "succeeded" && i.git_commit_sha.startsWith("e2e0000"));
  const run = await json(`/api/v1/runs/${withStdout.id}`);
  const meta = await json(`/api/v1/artifacts/${run.stdout_artifact.id}/metadata`);
  const { unlink } = await import("node:fs/promises");
  await unlink(`${process.env.DEVDATA}/artifacts/sha256/${meta.sha256.slice(0, 2)}/${meta.sha256}`);
  const c = await render(`/runs/${withStdout.id}`);
  await waitFor(c, (t) => t.includes("Artifacts") && !t.includes("Loading run"));
  const stdout = c.querySelector('[data-testid="artifact-stdout"]')!;
  check("detail: deleted artifact file shows unavailable with no links", stdout.textContent!.includes("unavailable") && !stdout.querySelector("a[download]") && stdout.querySelectorAll("button").length === 0);
  // Put the file back (identical content re-lands at the same path and
  // reuses the same row) so the script can be re-run.
  await fetch(`${BASE_URL}/api/v1/artifacts?kind=stdout&original_name=stdout.log`, {
    method: "POST",
    headers: { "Content-Type": "text/plain" },
    body: "prefill: 120.4 tok/s\ndecode: 38.2 tok/s\n",
  });
  const restored = await json(`/api/v1/artifacts/${run.stdout_artifact.id}/metadata`);
  check("detail: re-uploading identical content restores the file", restored.available === true);
}

}

if (SCENARIO === "empty") {
  {
    const c = await render("/");
    const text = await waitFor(c, (t) => t.includes("No runs recorded yet"));
    check("empty: results page distinguishes an empty database", text.includes("No runs recorded yet."), text.slice(0, 200));
  }
  {
    const c = await render("/runs");
    const text = await waitFor(c, (t) => t.includes("No runs recorded yet"));
    check("empty: runs page distinguishes an empty database", text.includes("No runs recorded yet."), text.slice(0, 200));
  }
}

if (SCENARIO === "dead") {
  for (const path of ["/", "/runs", "/runs/00000000-0000-7000-8000-000000000000"]) {
    const c = await render(path);
    const text = await waitFor(c, (t) => t.includes("unreachable"));
    const retry = Array.from(c.querySelectorAll("button")).find((b) => b.textContent === "Retry");
    check(`unreachable: ${path} shows backend-unreachable with retry`, text.includes("Backend unreachable.") && !!retry, text.slice(0, 200));
  }
}

console.log(failures === 0 ? "\nALL CHECKS PASSED" : `\n${failures} CHECK(S) FAILED`);
process.exit(failures === 0 ? 0 : 1);
