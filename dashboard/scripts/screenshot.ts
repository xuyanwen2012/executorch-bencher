// Captures the dashboard pages at a wide and a narrow viewport for visual
// review. Run against the live dev server:
//   SHOT_DIR=/tmp/shots bun scripts/screenshot.ts
// Requires `bun add -d playwright` and `bunx playwright install chromium`.
import { mkdir } from "node:fs/promises";
import { chromium } from "playwright";

const BASE = process.env.SHOT_BASE ?? "http://127.0.0.1:3111";
const OUT = process.env.SHOT_DIR ?? "./shots";
const TAG = process.env.SHOT_TAG ?? "";

const WIDTHS = [
  { name: "wide", width: 1440, height: 900 },
  { name: "narrow", width: 800, height: 1000 },
];

async function main() {
  await mkdir(OUT, { recursive: true });

  const androidRun = await pick(`${BASE}/api/v1/runs?platform=android&exit_status=succeeded&limit=1`);
  const crashedRun = await pick(`${BASE}/api/v1/runs?exit_status=crashed&limit=1`);
  const linuxRun = await pick(`${BASE}/api/v1/runs?platform=linux&limit=1`);
  const externalRun = await pick(`${BASE}/api/v1/runs?platform=android&device_class=external&limit=1`);

  const pages: { name: string; path: string }[] = [
    { name: "results", path: "/" },
    { name: "results-filtered", path: "/?platform=linux" },
    { name: "results-showall", path: "/?all=1&metric=decode" },
    { name: "results-narrowed", path: "/?platform=android&device_class=internal&model_asset_id=01a05f21-3746-7f71-bdc4-e25add7e4c10" },
    { name: "results-empty", path: "/?device_serial=nope-nothing-here" },
    { name: "runs", path: "/runs" },
    { name: "runs-linked", path: "/runs?platform=android&device_serial=R5CX12ABCDE&gpu_clock_mhz=980&mif_clock_mhz=5333&int_clock_mhz=934&prompt_sha256=a7b07eacd9ed7b4f57adbaeeff76155813b3f6e75d0351e1d7c9143c07228a96" },
    { name: "run-android", path: `/runs/${androidRun}` },
    { name: "run-linux", path: `/runs/${linuxRun}` },
    { name: "run-external", path: `/runs/${externalRun}` },
    { name: "run-crashed", path: `/runs/${crashedRun}` },
    { name: "run-missing", path: "/runs/00000000-0000-0000-0000-000000000000" },
    { name: "run-error", path: "/runs/not-a-uuid" },
  ];

  const browser = await chromium.launch();
  for (const size of WIDTHS) {
    const context = await browser.newContext({
      viewport: { width: size.width, height: size.height },
      deviceScaleFactor: 1,
      timezoneId: "America/Los_Angeles",
    });
    const page = await context.newPage();
    for (const spec of pages) {
      await page.goto(`${BASE}${spec.path}`, { waitUntil: "networkidle" });
      await page.waitForTimeout(350);
      const overflow = await page.evaluate(
        () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
      );
      if (overflow > 0) console.warn(`  ! ${spec.name} @${size.name}: page scrolls ${overflow}px horizontally`);
      const file = `${OUT}/${TAG}${spec.name}-${size.name}.png`;
      await page.screenshot({ path: file, fullPage: true });
      console.log(`  ${file}`);
    }
    await context.close();
  }
  await browser.close();
}

async function pick(url: string): Promise<string> {
  const res = await fetch(url);
  const body = (await res.json()) as { items: { id: string }[] };
  const id = body.items[0]?.id;
  if (!id) throw new Error(`no run from ${url}`);
  return id;
}

await main();
