import { describe, expect, test } from "bun:test";
import { renderToString } from "react-dom/server";
import { SpreadBar, SpreadLegend } from "./SpreadBar";

describe("SpreadBar", () => {
  test("renders nothing without stats or with a zero median", () => {
    expect(renderToString(<SpreadBar stats={null} tone="prefill" />)).toBe("");
    expect(renderToString(<SpreadBar stats={{ median: 0, min: 0, max: 0, n: 2 }} tone="prefill" />)).toBe("");
  });

  test("is an image labelled with the spread", () => {
    const html = renderToString(<SpreadBar stats={{ median: 100, min: 95, max: 110, n: 3 }} tone="decode" />);
    expect(html).toContain('role="img"');
    expect(html).toContain('aria-label="n=3: min −5.0%, max +10.0% of the median"');
    expect(html).toContain("bg-decode");
  });

  test("a single measurement draws the tick only", () => {
    const html = renderToString(<SpreadBar stats={{ median: 50, min: 50, max: 50, n: 1 }} tone="prefill" />);
    expect(html).toContain('aria-label="n=1: every measurement equal"');
    expect(html).not.toContain("rounded-[1px]");
  });

  test("a spread past full scale is clamped and marked", () => {
    const html = renderToString(<SpreadBar stats={{ median: 100, min: 70, max: 101, n: 4 }} tone="prefill" />);
    expect(html).toContain("rotate-45");
    expect(html).toContain("left-0");
    expect(html).not.toContain("right-0");
  });

  test("the legend explains the full-scale window", () => {
    expect(renderToString(<SpreadLegend tone="prefill" />)).toContain("±10%");
  });
});
