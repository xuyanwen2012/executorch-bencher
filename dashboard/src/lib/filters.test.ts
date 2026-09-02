import { describe, expect, test } from "bun:test";
import {
  hasAnyFilter,
  parseResultsFilters,
  parseRunsFilters,
  resultsFiltersToParams,
  resultsQuery,
  runsFiltersToParams,
  runsLinkForConfiguration,
  runsQuery,
} from "./filters";

describe("filters", () => {
  test("results filters round-trip through the URL, dropping empties", () => {
    const params = resultsFiltersToParams({ device_serial: "dev-1", git_dirty: "false", bsp_version: "" });
    expect(params.toString()).toBe("device_serial=dev-1&git_dirty=false");
    expect(parseResultsFilters(params)).toEqual({ device_serial: "dev-1", git_dirty: "false" });
    expect(parseResultsFilters(new URLSearchParams("unrelated=1&device_serial="))).toEqual({});
  });

  test("runs filters round-trip and type their query", () => {
    const filters = parseRunsFilters(
      new URLSearchParams("device_serial=d&gpu_clock_mhz=980&git_dirty=true&exit_status=crashed&prompt_sha256=abc"),
    );
    expect(runsFiltersToParams(filters).toString()).toBe(
      "device_serial=d&git_dirty=true&gpu_clock_mhz=980&prompt_sha256=abc&exit_status=crashed",
    );
    expect(runsQuery(filters)).toMatchObject({
      device_serial: "d",
      gpu_clock_mhz: 980,
      git_dirty: true,
      exit_status: "crashed",
      mif_clock_mhz: undefined,
    });
    expect(runsQuery({ gpu_clock_mhz: "not-a-number", git_dirty: "maybe", exit_status: "exploded" })).toMatchObject({
      gpu_clock_mhz: undefined,
      git_dirty: undefined,
      exit_status: undefined,
    });
  });

  test("results query parses the dirty flag", () => {
    expect(resultsQuery({ git_dirty: "true", model_asset_id: "m" })).toMatchObject({ git_dirty: true, model_asset_id: "m" });
    expect(resultsQuery({})).toMatchObject({ git_dirty: undefined });
  });

  test("device class is filtered on both pages and only for known values", () => {
    expect(resultsQuery({ device_class: "external" })).toMatchObject({ device_class: "external" });
    expect(runsQuery({ device_class: "internal" })).toMatchObject({ device_class: "internal" });
    expect(runsQuery({ device_class: "borrowed" })).toMatchObject({ device_class: undefined });
  });

  test("hasAnyFilter", () => {
    expect(hasAnyFilter({})).toBe(false);
    expect(hasAnyFilter({ a: "" })).toBe(false);
    expect(hasAnyFilter({ a: "x" })).toBe(true);
  });

  test("a results row links to exactly its configuration's runs", () => {
    const link = runsLinkForConfiguration({
      platform: "android",
      device_serial: "device-001",
      model_asset: { id: "m-1" },
      git_commit_sha: "abc123",
      git_dirty: true,
      sumd_driver_version: "sumd-1.0",
      bsp_version: "bsp-1.0",
      gpu_clock_mhz: 980,
      mif_clock_mhz: 5333,
      int_clock_mhz: 934,
      prompt_sha256: "c".repeat(64),
    });
    const params = new URLSearchParams(link.slice("/runs?".length));
    expect(parseRunsFilters(params)).toEqual({
      platform: "android",
      device_serial: "device-001",
      model_asset_id: "m-1",
      git_commit_sha: "abc123",
      git_dirty: "true",
      sumd_driver_version: "sumd-1.0",
      bsp_version: "bsp-1.0",
      gpu_clock_mhz: "980",
      mif_clock_mhz: "5333",
      int_clock_mhz: "934",
      prompt_sha256: "c".repeat(64),
    });
  });

  test("a linux results row links only with the dimensions it has", () => {
    const link = runsLinkForConfiguration({
      platform: "linux",
      device_serial: "ubuntu-lts-gpu",
      model_asset: { id: "m-1" },
      git_commit_sha: "abc123",
      git_dirty: false,
      sumd_driver_version: null,
      bsp_version: null,
      gpu_clock_mhz: null,
      mif_clock_mhz: null,
      int_clock_mhz: null,
      host_accelerator: "Intel(R) Arc(tm) B580 Graphics (BMG G21)",
      prompt_sha256: "c".repeat(64),
    });
    const params = new URLSearchParams(link.slice("/runs?".length));
    expect(parseRunsFilters(params)).toEqual({
      platform: "linux",
      device_serial: "ubuntu-lts-gpu",
      model_asset_id: "m-1",
      git_commit_sha: "abc123",
      git_dirty: "false",
      host_accelerator: "Intel(R) Arc(tm) B580 Graphics (BMG G21)",
      prompt_sha256: "c".repeat(64),
    });
    expect(runsQuery(parseRunsFilters(new URLSearchParams("platform=ios")))).toMatchObject({ platform: undefined });
  });
});
