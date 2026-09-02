// URL-backed filter state for the results and runs pages. Filters live in
// the query string so a filtered view can be shared or reloaded, and a
// results row can link to exactly its runs by passing its full key.
import type { components } from "../api/client";

// The enumerations come from the generated contract. Each runtime array is
// checked against its schema type both ways: a variant missing from the
// object literal is a missing property, an extra one is an excess property,
// so adding a value to the backend enum fails `tsc` here until it is listed.
export type Platform = components["schemas"]["Platform"];
export type DeviceClass = components["schemas"]["DeviceClass"];
export type ExitStatus = components["schemas"]["ExitStatus"];
export type CorrectnessResult = components["schemas"]["CorrectnessResult"];

/** Every variant of a string union, in the order the caller lists them. */
function variants<T extends string>(all: Record<T, true>): readonly T[] {
  return Object.keys(all) as T[];
}

export const PLATFORMS = variants<Platform>({ android: true, linux: true });

/** How much the device let the collector see: `internal` is a lab device
 * with the full BSP/driver/clock snapshot, `external` a retail phone or a
 * Linux box where those dimensions do not exist. */
export const DEVICE_CLASSES = variants<DeviceClass>({ internal: true, external: true });

export const EXIT_STATUSES = variants<ExitStatus>({
  succeeded: true,
  crashed: true,
  timed_out: true,
  cancelled: true,
  infrastructure_error: true,
});

export const CORRECTNESS_RESULTS = variants<CorrectnessResult>({
  passed: true,
  failed: true,
  not_checked: true,
  validator_error: true,
});

export const RESULTS_FILTER_KEYS = [
  "platform",
  "device_class",
  "device_serial",
  "model_asset_id",
  "git_branch",
  "git_dirty",
  "sumd_driver_version",
  "bsp_version",
  "host_accelerator",
] as const;
export type ResultsFilterKey = (typeof RESULTS_FILTER_KEYS)[number];
export type ResultsFilters = Partial<Record<ResultsFilterKey, string>>;

export const RUNS_FILTER_KEYS = [
  "platform",
  "device_class",
  "device_serial",
  "model_asset_id",
  "git_commit_sha",
  "git_branch",
  "git_dirty",
  "sumd_driver_version",
  "bsp_version",
  "gpu_clock_mhz",
  "mif_clock_mhz",
  "int_clock_mhz",
  "host_accelerator",
  "prompt_sha256",
  "exit_status",
  "correctness_result",
] as const;
export type RunsFilterKey = (typeof RUNS_FILTER_KEYS)[number];
export type RunsFilters = Partial<Record<RunsFilterKey, string>>;

function parse<K extends string>(params: URLSearchParams, keys: readonly K[]): Partial<Record<K, string>> {
  const out: Partial<Record<K, string>> = {};
  for (const key of keys) {
    const value = params.get(key);
    if (value !== null && value !== "") out[key] = value;
  }
  return out;
}

function serialize<K extends string>(filters: Partial<Record<K, string>>, keys: readonly K[]): URLSearchParams {
  const params = new URLSearchParams();
  for (const key of keys) {
    const value = filters[key];
    if (value !== undefined && value !== "") params.set(key, value);
  }
  return params;
}

export function parseResultsFilters(params: URLSearchParams): ResultsFilters {
  return parse(params, RESULTS_FILTER_KEYS);
}

export function resultsFiltersToParams(filters: ResultsFilters): URLSearchParams {
  return serialize(filters, RESULTS_FILTER_KEYS);
}

export function parseRunsFilters(params: URLSearchParams): RunsFilters {
  return parse(params, RUNS_FILTER_KEYS);
}

export function runsFiltersToParams(filters: RunsFilters): URLSearchParams {
  return serialize(filters, RUNS_FILTER_KEYS);
}

export function hasAnyFilter(filters: Record<string, string | undefined>): boolean {
  return Object.values(filters).some((v) => v !== undefined && v !== "");
}

function oneOf<T extends string>(values: readonly T[], value: string | undefined): T | undefined {
  return values.find((v) => v === value);
}

function parseBool(value: string | undefined): boolean | undefined {
  if (value === "true") return true;
  if (value === "false") return false;
  return undefined;
}

function parseInt64(value: string | undefined): number | undefined {
  if (value === undefined) return undefined;
  const n = Number(value);
  return Number.isInteger(n) ? n : undefined;
}

/** Typed query object for `GET /api/v1/results`. */
export function resultsQuery(filters: ResultsFilters) {
  return {
    platform: oneOf(PLATFORMS, filters.platform),
    device_class: oneOf(DEVICE_CLASSES, filters.device_class),
    device_serial: filters.device_serial,
    model_asset_id: filters.model_asset_id,
    git_branch: filters.git_branch,
    git_dirty: parseBool(filters.git_dirty),
    sumd_driver_version: filters.sumd_driver_version,
    bsp_version: filters.bsp_version,
    host_accelerator: filters.host_accelerator,
  };
}

/** Typed query object for `GET /api/v1/runs` (without paging). */
export function runsQuery(filters: RunsFilters) {
  return {
    platform: oneOf(PLATFORMS, filters.platform),
    device_class: oneOf(DEVICE_CLASSES, filters.device_class),
    device_serial: filters.device_serial,
    model_asset_id: filters.model_asset_id,
    git_commit_sha: filters.git_commit_sha,
    git_branch: filters.git_branch,
    git_dirty: parseBool(filters.git_dirty),
    sumd_driver_version: filters.sumd_driver_version,
    bsp_version: filters.bsp_version,
    gpu_clock_mhz: parseInt64(filters.gpu_clock_mhz),
    mif_clock_mhz: parseInt64(filters.mif_clock_mhz),
    int_clock_mhz: parseInt64(filters.int_clock_mhz),
    host_accelerator: filters.host_accelerator,
    prompt_sha256: filters.prompt_sha256,
    exit_status: oneOf(EXIT_STATUSES, filters.exit_status),
    correctness_result: oneOf(CORRECTNESS_RESULTS, filters.correctness_result),
  };
}

/** The configuration key fields a results row links to the runs page with.
 * The platform-specific dimensions are null on the other platform. */
export interface ConfigurationKey {
  platform: Platform;
  device_serial: string;
  model_asset: { id: string };
  git_commit_sha: string;
  git_dirty: boolean;
  sumd_driver_version?: string | null;
  bsp_version?: string | null;
  gpu_clock_mhz?: number | null;
  mif_clock_mhz?: number | null;
  int_clock_mhz?: number | null;
  host_accelerator?: string | null;
  prompt_sha256: string;
}

function ifSet(value: string | number | null | undefined): string | undefined {
  return value === null || value === undefined ? undefined : String(value);
}

/** `/runs?...` filtered to exactly one configuration's runs. */
export function runsLinkForConfiguration(key: ConfigurationKey): string {
  const params = runsFiltersToParams({
    platform: key.platform,
    device_serial: key.device_serial,
    model_asset_id: key.model_asset.id,
    git_commit_sha: key.git_commit_sha,
    git_dirty: String(key.git_dirty),
    sumd_driver_version: ifSet(key.sumd_driver_version),
    bsp_version: ifSet(key.bsp_version),
    gpu_clock_mhz: ifSet(key.gpu_clock_mhz),
    mif_clock_mhz: ifSet(key.mif_clock_mhz),
    int_clock_mhz: ifSet(key.int_clock_mhz),
    host_accelerator: ifSet(key.host_accelerator),
    prompt_sha256: key.prompt_sha256,
  });
  return `/runs?${params.toString()}`;
}
