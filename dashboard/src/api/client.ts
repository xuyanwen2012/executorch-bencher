// Typed API client generated from ../../openapi/openapi.json.
// Regenerate the types with `bun run generate-api` after any backend
// contract change; `bun run check` fails when they are stale.
import createClient from "openapi-fetch";
import type { paths } from "./schema";

/** Same-origin: the dev server proxies /api to the backend, and in
 * production the backend serves the dashboard itself. */
export const api = createClient<paths>({ baseUrl: "" });

export type { components, paths } from "./schema";
