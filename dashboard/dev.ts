// Development server: serves the app with hot reload and proxies API
// requests to the backend so the browser only ever talks to one origin.
// See specs/benchmark-dashboard - "Development server proxies API requests
// to the backend".
import { serve } from "bun";
import index from "./src/index.html";

const BACKEND_URL = (process.env.BACKEND_URL ?? "http://127.0.0.1:3000").replace(/\/$/, "");
const PORT = Number(process.env.PORT ?? 3001);

/** Headers that describe one hop, not the message (RFC 9110 §7.6.1); a
 * proxy must not forward them. */
const HOP_BY_HOP = new Set([
  "connection",
  "keep-alive",
  "proxy-authenticate",
  "proxy-authorization",
  "te",
  "trailer",
  "transfer-encoding",
  "upgrade",
]);

/** Request headers to forward: everything but the hop-by-hop set and the
 * origin `Host`, which fetch derives from the target. */
function outgoingHeaders(req: Request): Headers {
  const headers = new Headers();
  req.headers.forEach((value, name) => {
    if (name === "host" || HOP_BY_HOP.has(name)) return;
    headers.append(name, value);
  });
  return headers;
}

/** Response headers to relay. Bun's fetch has already decoded the body, so
 * `content-encoding` and `content-length` would describe bytes the browser
 * never sees; dropping them lets the body stream through unbuffered, which
 * is what the SSE endpoint needs. */
function incomingHeaders(upstream: Response): Headers {
  const headers = new Headers();
  upstream.headers.forEach((value, name) => {
    if (HOP_BY_HOP.has(name) || name === "content-encoding" || name === "content-length") return;
    headers.append(name, value);
  });
  return headers;
}

/** Forwards `req` to the backend, streaming the body both ways. The
 * browser's abort signal is passed along so closing a tab closes the
 * backend's event-stream subscription instead of leaking it. */
async function proxy(req: Request): Promise<Response> {
  const incoming = new URL(req.url);
  const target = `${BACKEND_URL}${incoming.pathname}${incoming.search}`;
  try {
    const upstream = await fetch(target, {
      method: req.method,
      headers: outgoingHeaders(req),
      body: req.method === "GET" || req.method === "HEAD" ? undefined : req.body,
      redirect: "manual",
      signal: req.signal,
    });
    return new Response(upstream.body, {
      status: upstream.status,
      statusText: upstream.statusText,
      headers: incomingHeaders(upstream),
    });
  } catch (err) {
    if (req.signal.aborted) return new Response(null, { status: 499 });
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

// The backend also serves Swagger UI at /docs and the raw document at
// /openapi.json (the footer links there); they must be proxied explicitly
// or the SPA fallback would answer with index.html.
const PROXIED = ["/api/*", "/health", "/docs", "/docs/*", "/openapi.json"] as const;

const server = serve({
  port: PORT,
  routes: {
    ...Object.fromEntries(PROXIED.map((route) => [route, proxy])),
    "/*": index,
  },
  development: {
    hmr: true,
    console: true,
  },
});

console.log(`dashboard dev server at ${server.url} (proxying ${PROXIED.join(", ")} to ${BACKEND_URL})`);
