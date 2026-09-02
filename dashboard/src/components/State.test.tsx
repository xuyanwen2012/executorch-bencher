import { describe, expect, test } from "bun:test";
import { renderToString } from "react-dom/server";
import { ApiError } from "../api/errors";
import { click, mount } from "../../tests/render";
import { AbsentDash, Absent, ErrorState, Loading } from "./State";

describe("State", () => {
  test("Loading is announced as a status", () => {
    const html = renderToString(<Loading label="Loading runs…" />);
    expect(html).toContain('role="status"');
    expect(html).toContain("Loading runs…");
  });

  test("an unreachable backend shows the unreachable message and a working retry", async () => {
    let retries = 0;
    const error = new ApiError({ kind: "unreachable", message: "backend could not be reached: ECONNREFUSED" });
    const m = await mount(<ErrorState error={error} onRetry={() => retries++} />);
    expect(m.container.querySelector('[role="alert"]')).not.toBeNull();
    expect(m.container.textContent).toContain("Backend unreachable");
    expect(m.container.textContent).toContain("ECONNREFUSED");
    const retry = Array.from(m.container.querySelectorAll("button")).find((b) => b.textContent === "Retry")!;
    await click(retry);
    expect(retries).toBe(1);
    await m.unmount();
  });

  test("an API error shows its code and status", () => {
    const error = new ApiError({ kind: "api", status: 400, code: "invalid_filter", message: "bad cursor" });
    const html = renderToString(<ErrorState error={error} onRetry={() => {}} />);
    expect(html).toContain("HTTP 400");
    expect(html).toContain("invalid_filter");
    expect(html).toContain("bad cursor");
  });

  test("absent markers carry their reason for assistive technology", () => {
    const dash = renderToString(<AbsentDash />);
    expect(dash).toContain('role="img"');
    expect(dash).toContain('aria-label="not recorded"');
    expect(renderToString(<AbsentDash title="Not applicable on this platform" />)).toContain(
      'aria-label="Not applicable on this platform"',
    );
    const absent = renderToString(<Absent title="No succeeded run recorded this phase." className="block" />);
    expect(absent).toContain("not recorded");
    expect(absent).toContain('title="No succeeded run recorded this phase."');
  });
});
