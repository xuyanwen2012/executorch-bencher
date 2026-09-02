import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter, Route, Routes, useLocation, useNavigationType } from "react-router";
import { api } from "../api/client";
import { mount, select, waitFor } from "../../tests/render";
import { RunsPage } from "./RunsPage";

/** Answers the runs and models calls locally with empty lists. */
const middleware = {
  onRequest({ request }: { request: Request }) {
    const url = new URL(request.url, "http://test.local");
    const body = url.pathname === "/api/v1/models" ? [] : { items: [], next_cursor: null };
    return new Response(JSON.stringify(body), { status: 200, headers: { "content-type": "application/json" } });
  },
};
beforeEach(() => api.use(middleware));
afterEach(() => api.eject(middleware));

/** Records how the last navigation happened, for the assertions below. */
function Probe() {
  const location = useLocation();
  const type = useNavigationType();
  return <output data-testid="probe" data-search={location.search} data-type={type} />;
}

function app(initial = "/runs") {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return (
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[initial]}>
        <Routes>
          <Route
            path="/runs"
            element={
              <>
                <RunsPage />
                <Probe />
              </>
            }
          />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>
  );
}

describe("RunsPage", () => {
  test("a filter edit replaces the history entry instead of pushing one", async () => {
    const m = await mount(app());
    const probe = () => m.container.querySelector('[data-testid="probe"]')!;
    await waitFor(() => (m.container.textContent ?? "").includes("No runs recorded yet."));
    expect(probe().getAttribute("data-type")).toBe("POP");

    const platform = m.container.querySelector<HTMLSelectElement>("#filter-platform")!;
    await select(platform, "linux");
    expect(probe().getAttribute("data-search")).toBe("?platform=linux");
    expect(probe().getAttribute("data-type")).toBe("REPLACE");
    await waitFor(() => (m.container.textContent ?? "").includes("No runs match these filters."));
    await m.unmount();
  });

  test("the list is not called complete before it has loaded", async () => {
    const m = await mount(app());
    // While pending the status slot is empty rather than "0 runs" or "All".
    const status = m.container.querySelector('[data-testid="filter-status"]')!;
    expect(status.textContent).toBe("");
    expect(m.container.textContent).not.toContain("matching these filters are listed");
    await waitFor(() => (m.container.textContent ?? "").includes("No runs recorded yet."));
    await m.unmount();
  });
});
