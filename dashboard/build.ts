// Production build. Always use this (bun run build), never bare `bun build`:
// the CLI ignores bunfig.toml plugins and would leave Tailwind directives
// unprocessed in the emitted CSS.
import tailwind from "bun-plugin-tailwind";
import { rm } from "node:fs/promises";
import path from "node:path";

const outdir = path.join(import.meta.dir, "dist");

// Source maps default to "external" (a .map file beside each bundle, no
// sourceMappingURL comment in the served asset). SOURCEMAP=none|linked|
// inline|external overrides it for debugging a production bundle.
const SOURCEMAPS = ["none", "linked", "inline", "external"] as const;
type Sourcemap = (typeof SOURCEMAPS)[number];
const requested = process.env.SOURCEMAP ?? "external";
if (!(SOURCEMAPS as readonly string[]).includes(requested)) {
  console.error(`SOURCEMAP must be one of ${SOURCEMAPS.join(", ")}, got ${JSON.stringify(requested)}`);
  process.exit(2);
}
const sourcemap = requested as Sourcemap;
await rm(outdir, { recursive: true, force: true });

const result = await Bun.build({
  entrypoints: [path.join(import.meta.dir, "src/index.html")],
  outdir,
  plugins: [tailwind],
  minify: true,
  target: "browser",
  sourcemap,
  define: {
    "process.env.NODE_ENV": JSON.stringify("production"),
  },
});

if (!result.success) {
  for (const log of result.logs) {
    console.error(log);
  }
  process.exit(1);
}

for (const output of result.outputs) {
  console.log(` ${path.relative(import.meta.dir, output.path)}  ${(output.size / 1024).toFixed(1)} KB`);
}
