import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderToString } from "react-dom/server";
import { MemoryRouter, Route, Routes } from "react-router";
import { api } from "../api/client";
import { act, mount, waitFor } from "../../tests/render";
import { Layout, LiveIndicator } from "./Layout";

/** A scriptable stand-in for the browser's EventSource. */
class FakeEventSource {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSED = 2;
  static instances: FakeEventSource[] = [];
  readyState = 0;
  onopen: ((event: Event) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;
  listeners = new Map<string, ((event: MessageEvent<string>) => void)[]>();
  constructor(public url: string) {
    FakeEventSource.instances.push(this);
  }
  addEventListener(type: string, listener: (event: MessageEvent<string>) => void) {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), listener]);
  }
  close() {
    this.readyState = 2;
  }
  open() {
    this.readyState = 1;
    this.onopen?.(new Event("open"));
  }
  fail(readyState: number) {
    this.readyState = readyState;
    this.onerror?.(new Event("error"));
  }
}

const originalEventSource = globalThis.EventSource;

beforeEach(() => {
  FakeEventSource.instances = [];
  globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
});
afterEach(() => {
  globalThis.EventSource = originalEventSource;
});

/** The version call is answered locally: 200 in `ok`, 503 otherwise. */
function stubVersion(ok: boolean) {
  const middleware = {
    onRequest({ request }: { request: Request }) {
      if (!request.url.includes("/api/v1/version")) return undefined;
      const body = ok
        ? { api_version: "1.1", schema_version: 7, server_version: "0.3.0" }
        : { error: { code: "unavailable", message: "down" } };
      return new Response(JSON.stringify(body), {
        status: ok ? 200 : 503,
        headers: { "content-type": "application/json" },
      });
    },
  };
  api.use(middleware);
  return () => api.eject(middleware);
}

function app() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return (
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={["/"]}>
        <Routes>
          <Route element={<Layout />}>
            <Route index element={<p>page body</p>} />
          </Route>
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>
  );
}

describe("LiveIndicator", () => {
  test("each state has its own label, dot and honest explanation", () => {
    const live = renderToString(<LiveIndicator state="live" />);
    expect(live).toContain('data-state="live"');
    expect(live).toContain(">live<");
    expect(live).toContain("bg-ok");
    expect(live).toContain('aria-live="polite"');
    expect(live).toContain('role="status"');
    const connecting = renderToString(<LiveIndicator state="connecting" />);
    expect(connecting).toContain(">connecting<");
    expect(connecting).toContain("bg-warn");
    const off = renderToString(<LiveIndicator state="off" />);
    expect(off).toContain("live updates off");
    expect(off).toContain("reopens the stream on its own");
    expect(off).toContain("up to 30 s");
  });
});

describe("Layout", () => {
  test("mirrors the event stream: connecting, live, then off when it closes", async () => {
    const eject = stubVersion(true);
    const m = await mount(app());
    const indicator = () => m.container.querySelector('[data-testid="live-indicator"]')!;
    expect(FakeEventSource.instances).toHaveLength(1);
    expect(FakeEventSource.instances[0]!.url).toBe("/api/v1/events");
    expect(indicator().getAttribute("data-state")).toBe("connecting");

    await act(async () => FakeEventSource.instances[0]!.open());
    expect(indicator().getAttribute("data-state")).toBe("live");

    await act(async () => FakeEventSource.instances[0]!.fail(FakeEventSource.CONNECTING));
    expect(indicator().getAttribute("data-state")).toBe("connecting");

    await act(async () => FakeEventSource.instances[0]!.fail(FakeEventSource.CLOSED));
    expect(indicator().getAttribute("data-state")).toBe("off");
    expect(indicator().getAttribute("title")).toContain("reopens the stream on its own");

    await m.unmount();
    eject();
  });

  test("the footer shows the backend's versions", async () => {
    const eject = stubVersion(true);
    const m = await mount(app());
    await waitFor(() => (m.container.textContent ?? "").includes("api 1.1"));
    expect(m.container.textContent).toContain("schema v7");
    expect(m.container.textContent).toContain("server 0.3.0");
    expect(m.container.querySelector('a[href="/docs"]')).not.toBeNull();
    await m.unmount();
    eject();
  });

  test("the footer says so when the version call fails", async () => {
    const eject = stubVersion(false);
    const m = await mount(app());
    await waitFor(() => m.container.querySelector('[data-testid="version-unavailable"]') !== null);
    expect(m.container.textContent).toContain("version unavailable");
    await m.unmount();
    eject();
  });
});
