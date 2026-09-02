// Display formatting. Every helper returns `null` for an absent value so
// callers render an explicit absent marker, never a blank or a zero.

export const ABSENT = "not recorded";

/** Short SHA length used everywhere a commit is identified. */
const SHORT_SHA = 10;

/** The bare short SHA; the dirty state is carried by a badge beside it. */
export function abbreviateSha(sha: string): string {
  return sha.length > SHORT_SHA ? sha.slice(0, SHORT_SHA) : sha;
}

/** Model label: the registered `.pte` filename without its extension. */
export function modelLabel(originalName: string): string {
  return originalName.replace(/\.pte$/i, "");
}

export function formatTokPerSec(value: number | null | undefined): string | null {
  if (value === null || value === undefined || !Number.isFinite(value)) return null;
  return value.toFixed(1);
}

export interface StatsLike {
  median: number;
  min: number;
  max: number;
  n: number;
}

/**
 * The min–max range on its own, for tables that carry `n` in a column of
 * its own. `n` is appended only when it differs from `baselineN`, which is
 * the case when one phase was recorded for fewer runs than the other.
 */
export function formatRange(stats: StatsLike | null | undefined, baselineN?: number): string | null {
  if (!stats) return null;
  const range = stats.min === stats.max ? stats.min.toFixed(1) : `${stats.min.toFixed(1)}–${stats.max.toFixed(1)}`;
  return baselineN !== undefined && stats.n !== baselineN ? `${range} (n=${stats.n})` : range;
}

/** Normalizes any RFC 3339 timestamp to a `Z`-suffixed UTC string. */
export function utcIso(iso: string | null | undefined): string | null {
  if (!iso) return null;
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  return date.toISOString();
}

// Local time is rendered ISO-style (`2026-09-01 14:12:35`) rather than in
// the reader's locale order: it is the convention of the logs this data
// comes from, it sorts by eye down a column, and it is the same width in
// every row. The zone is still the browser's own; the UTC form is on hover.
const localFormatter = new Intl.DateTimeFormat("en-CA", {
  year: "numeric",
  month: "2-digit",
  day: "2-digit",
  hour: "2-digit",
  minute: "2-digit",
  second: "2-digit",
  hourCycle: "h23",
});

/** The browser's local rendering of a UTC timestamp, `YYYY-MM-DD HH:MM:SS`. */
export function formatLocalTime(iso: string | null | undefined): string | null {
  if (!iso) return null;
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  return localFormatter.format(date).replace(", ", " ");
}

/** The date and the clock time separately, for two-line table cells. */
export function splitLocalTime(iso: string | null | undefined): { date: string; time: string } | null {
  const local = formatLocalTime(iso);
  if (!local) return null;
  const [date, time] = local.split(" ");
  return date && time ? { date, time } : null;
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KiB", "MiB", "GiB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value >= 10 ? 0 : 1)} ${units[unit]}`;
}

export function formatDurationSeconds(seconds: number): string {
  if (seconds < 60) return `${seconds} s`;
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = seconds % 60;
  return h > 0 ? `${h}h ${m}m ${s}s` : `${m}m ${s}s`;
}

/** Wall-clock time between two RFC 3339 stamps, or null if not computable. */
export function formatElapsed(startIso: string | null | undefined, endIso: string | null | undefined): string | null {
  if (!startIso || !endIso) return null;
  const start = new Date(startIso).getTime();
  const end = new Date(endIso).getTime();
  if (Number.isNaN(start) || Number.isNaN(end) || end < start) return null;
  const ms = end - start;
  if (ms < 1000) return `${ms} ms`;
  return formatDurationSeconds(Math.round(ms / 1000));
}

export function formatCount(n: number): string {
  return n.toLocaleString();
}

/** "1 run" / "12 runs". */
export function pluralRuns(n: number): string {
  return `${formatCount(n)} ${n === 1 ? "run" : "runs"}`;
}
