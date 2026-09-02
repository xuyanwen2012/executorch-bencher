import { describe, expect, test } from "bun:test";
import { FULL_SCALE, describeSpread, spreadGeometry } from "./spread";

describe("spreadGeometry", () => {
  test("a symmetric spread sits symmetrically around the centre", () => {
    const geometry = spreadGeometry({ median: 100, min: 95, max: 105, n: 3 });
    expect(geometry).not.toBeNull();
    expect(geometry?.leftPct).toBeCloseTo(25);
    expect(geometry?.rightPct).toBeCloseTo(75);
    expect(geometry?.medianPct).toBe(50);
    expect(geometry?.clampedLow).toBe(false);
    expect(geometry?.clampedHigh).toBe(false);
    expect(geometry?.single).toBe(false);
  });

  test("deviation beyond full scale is clamped to the track and reported", () => {
    const geometry = spreadGeometry({ median: 1049.2, min: 719.8, max: 1075.3, n: 3 });
    expect(geometry?.leftPct).toBe(0);
    expect(geometry?.clampedLow).toBe(true);
    expect(geometry?.clampedHigh).toBe(false);
    expect(geometry?.deviation).toBeGreaterThan(FULL_SCALE);
  });

  test("one measurement, or an identical min and max, draws a tick only", () => {
    expect(spreadGeometry({ median: 133, min: 133, max: 133, n: 1 })?.single).toBe(true);
    expect(spreadGeometry({ median: 133, min: 133, max: 133, n: 4 })?.single).toBe(true);
  });

  test("there is no relative scale without a positive median", () => {
    expect(spreadGeometry(null)).toBeNull();
    expect(spreadGeometry(undefined)).toBeNull();
    expect(spreadGeometry({ median: 0, min: 0, max: 0, n: 2 })).toBeNull();
  });
});

describe("describeSpread", () => {
  test("states the spread as a share of the median", () => {
    expect(describeSpread({ median: 100, min: 95, max: 110, n: 3 })).toBe(
      "n=3: min −5.0%, max +10.0% of the median",
    );
  });

  test("a single value says so", () => {
    expect(describeSpread({ median: 100, min: 100, max: 100, n: 1 })).toBe("n=1: every measurement equal");
    expect(describeSpread(null)).toBeNull();
  });
});
