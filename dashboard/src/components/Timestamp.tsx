import { formatLocalTime, splitLocalTime, utcIso } from "../lib/format";
import { Absent } from "./State";

/**
 * A timestamp in the browser's own zone, with the full UTC RFC 3339 value
 * on hover. `both` prints the UTC form alongside (the run detail view);
 * `stacked` puts the clock time under the date, for narrow table columns.
 * See specs/benchmark-dashboard - "Dashboard presents times in local time
 * with UTC available".
 */
export function Timestamp({
  iso,
  both = false,
  stacked = false,
  timeOnly = false,
}: {
  iso: string | null | undefined;
  both?: boolean;
  stacked?: boolean;
  /** Clock time alone, for a table whose rows are grouped under a date. */
  timeOnly?: boolean;
}) {
  const local = formatLocalTime(iso);
  const utc = utcIso(iso);
  if (!local || !utc) return <Absent />;
  const parts = splitLocalTime(iso);

  if (timeOnly && parts) {
    return (
      <span className="font-mono whitespace-nowrap" title={`${local} local · ${utc} UTC`}>
        {parts.time}
      </span>
    );
  }

  if (stacked && parts) {
    return (
      <span className="block font-mono" title={`${local} local · ${utc} UTC`}>
        <span className="block text-ink-2">{parts.date}</span>
        <span className="block text-ink-3">{parts.time}</span>
      </span>
    );
  }

  return (
    <span title={`${local} local · ${utc} UTC`} className="whitespace-nowrap">
      <span className="font-mono">{local}</span>
      {both ? <span className="ml-2 font-mono text-xs text-ink-3">{utc} UTC</span> : null}
    </span>
  );
}
