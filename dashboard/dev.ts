// Development server: serves the app with hot reload and proxies API
// requests to the backend so the browser only ever talks to one origin.
// See specs/benchmark-dashboard - "Development server proxies API requests
// to the backend".
import { serve } from "bun";
import index from "./src/index.html";

const BACKEND_URL = (process.env.BACKEND_URL ?? "http://127.0.0.1:3000").replace(/\/$/, "");
const PORT = Number(process.env.PORT ?? 3001);

/** Forwards `req` to the backend unchanged, streaming the body both ways. */
async function proxy(req: Request): Promise<Response> {
  const incoming = new URL(req.url);
  const target = `${BACKEND_URL}${incoming.pathname}${incoming.search}`;
  const headers = new Headers(req.headers);
  headers.delete("host");
  try {
    const upstream = await fetch(target, {
      method: req.method,
      headers,
      body: req.method === "GET" || req.method === "HEAD" ? undefined : req.body,
      redirect: "manual",
    });
    return new Response(upstream.body, {
      status: upstream.status,
      statusText: upstream.statusText,
      headers: upstream.headers,
    });
  } catch (err) {
    return Response.json(
      {
        error: {
          code: "backend_unreachable",
          message: `dev proxy could not reach ${BACKEND_URL}: ${String(err)}`,
        },
      },
      { status: 502 },
    );
  }
}

const server = serve({
  port: PORT,
  routes: {
    "/api/*": proxy,
    "/health": proxy,
    "/*": index,
  },
  development: {
    hmr: true,
    console: true,
  },
});

console.log(`dashboard dev server at ${server.url} (proxying /api and /health to ${BACKEND_URL})`);
