// Fails when src/api/schema.d.ts no longer matches what openapi-typescript
// would generate from ../openapi/openapi.json. Mirrors the backend's
// tests/openapi_drift.rs on the TypeScript side.
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";

const root = path.resolve(import.meta.dir, "..");
const spec = path.join(root, "..", "openapi", "openapi.json");
const checkedIn = path.join(root, "src", "api", "schema.d.ts");

const dir = await mkdtemp(path.join(tmpdir(), "api-drift-"));
const fresh = path.join(dir, "schema.d.ts");
try {
  const gen = Bun.spawnSync(["bunx", "openapi-typescript", spec, "-o", fresh], {
    cwd: root,
    stdout: "pipe",
    stderr: "pipe",
  });
  if (gen.exitCode !== 0) {
    console.error(gen.stderr.toString());
    process.exit(gen.exitCode);
  }
  const [expected, actual] = await Promise.all([readFile(fresh, "utf8"), readFile(checkedIn, "utf8")]);
  if (expected !== actual) {
    console.error(
      "src/api/schema.d.ts is out of date with ../openapi/openapi.json. Run `bun run generate-api` and commit the result.",
    );
    process.exit(1);
  }
  console.log("api types in sync with ../openapi/openapi.json");
} finally {
  await rm(dir, { recursive: true, force: true });
}
