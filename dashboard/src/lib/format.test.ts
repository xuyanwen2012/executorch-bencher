import { describe, expect, test } from "bun:test";
import {
  abbreviateSha,
  formatBytes,
  formatDurationSeconds,
  formatElapsed,
  formatLocalTime,
  formatRange,
  formatTokPerSec,
  modelLabel,
  pluralRuns,
  splitLocalTime,
  utcIso,
} from "./format";

describe("format", () => {
  test("sha is abbreviated to a fixed length", () => {
    expect(abbreviateSha("e2e0000000000000000000000000000deadbeef")).toBe("e2e0000000");
    expect(abbreviateSha("abc123")).toBe("abc123");
  });

  test("model label strips the .pte extension only", () => {
    expect(modelLabel("llama3_1_8b_4w.pte")).toBe("llama3_1_8b_4w");
    expect(modelLabel("model.PTE")).toBe("model");
    expect(modelLabel("weights.bin")).toBe("weights.bin");
  });

  test("throughput is one decimal and null stays null", () => {
    expect(formatTokPerSec(120.456)).toBe("120.5");
    expect(formatTokPerSec(0)).toBe("0.0");
    expect(formatTokPerSec(null)).toBeNull();
    expect(formatTokPerSec(undefined)).toBeNull();
    expect(formatTokPerSec(Number.NaN)).toBeNull();
  });

  test("range renders min–max, collapsing a single value", () => {
    expect(formatRange({ median: 120, min: 100, max: 900, n: 5 })).toBe("100.0–900.0");
    expect(formatRange({ median: 120, min: 120, max: 120, n: 1 })).toBe("120.0");
    expect(formatRange(null)).toBeNull();
  });

  test("range names n only when it differs from the row's own count", () => {
    expect(formatRange({ median: 1, min: 1, max: 2, n: 3 }, 3)).toBe("1.0–2.0");
    expect(formatRange({ median: 1, min: 1, max: 2, n: 2 }, 3)).toBe("1.0–2.0 (n=2)");
  });

  test("timestamps normalize to UTC and render locally, ISO-style", () => {
    expect(utcIso("2026-08-30T10:00:00+00:00")).toBe("2026-08-30T10:00:00.000Z");
    expect(utcIso(null)).toBeNull();
    expect(formatLocalTime("2026-08-30T10:00:00Z")).toMatch(/^2026-\d\d-\d\d \d\d:\d\d:\d\d$/);
    expect(formatLocalTime(undefined)).toBeNull();
  });

  test("a local timestamp splits into a date and a clock time", () => {
    const parts = splitLocalTime("2026-08-30T10:00:00Z");
    expect(parts?.date).toMatch(/^2026-\d\d-\d\d$/);
    expect(parts?.time).toMatch(/^\d\d:\d\d:\d\d$/);
    expect(splitLocalTime(null)).toBeNull();
  });

  test("bytes and durations", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1536)).toBe("1.5 KiB");
    expect(formatBytes(14 * 1024 * 1024 * 1024)).toBe("14 GiB");
    expect(formatDurationSeconds(42)).toBe("42 s");
    expect(formatDurationSeconds(3725)).toBe("1h 2m 5s");
  });

  test("elapsed time needs both ends and never runs backwards", () => {
    expect(formatElapsed("2026-08-30T10:00:00Z", "2026-08-30T10:01:00Z")).toBe("1m 0s");
    expect(formatElapsed("2026-08-30T10:00:00Z", "2026-08-30T10:00:00.250Z")).toBe("250 ms");
    expect(formatElapsed("2026-08-30T10:00:00Z", null)).toBeNull();
    expect(formatElapsed("2026-08-30T10:01:00Z", "2026-08-30T10:00:00Z")).toBeNull();
  });

  test("run counts read as prose", () => {
    expect(pluralRuns(1)).toBe("1 run");
    expect(pluralRuns(3)).toBe("3 runs");
  });
});
