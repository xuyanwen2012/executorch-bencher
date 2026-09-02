import { Badge, humanToken, type Tone } from "./State";

// One vocabulary of status chips, used identically on the results table,
// the runs table and the run detail page. Filled chips mean "look here":
// a run that did not succeed, a wrong answer, a throttled device, a build
// from a dirty working tree. Everything expected is quiet text.

export function exitTone(status: string): { tone: Tone; plain: boolean } {
  return status === "succeeded" ? { tone: "ok", plain: true } : { tone: "danger", plain: false };
}

export function correctnessTone(result: string): { tone: Tone; plain: boolean } {
  if (result === "failed") return { tone: "danger", plain: false };
  if (result === "validator_error") return { tone: "warn", plain: false };
  if (result === "passed") return { tone: "ok", plain: true };
  return { tone: "neutral", plain: true };
}

export function ExitBadge({ status }: { status: string }) {
  const { tone, plain } = exitTone(status);
  return (
    <Badge tone={tone} plain={plain} title={`Process exit status: ${humanToken(status)}`}>
      {humanToken(status)}
    </Badge>
  );
}

export function CorrectnessBadge({ result }: { result: string }) {
  const { tone, plain } = correctnessTone(result);
  return (
    <Badge tone={tone} plain={plain} title={`Output correctness: ${humanToken(result)}`}>
      {humanToken(result)}
    </Badge>
  );
}

export function ThrottledBadge() {
  return (
    <Badge tone="warn" title="The device reported thermal throttling during this run; its throughput is not comparable.">
      throttled
    </Badge>
  );
}

export function DirtyBadge() {
  return (
    <Badge tone="dirty" title="Built from a working tree with uncommitted changes; the commit alone does not identify this build.">
      dirty
    </Badge>
  );
}

const LEGEND: { badge: React.ReactNode; meaning: string }[] = [
  { badge: <ExitBadge status="crashed" />, meaning: "the process did not exit cleanly" },
  { badge: <CorrectnessBadge result="failed" />, meaning: "output did not match the expected answer" },
  { badge: <ThrottledBadge />, meaning: "the device throttled; throughput is not comparable" },
  { badge: <DirtyBadge />, meaning: "uncommitted changes in the working tree" },
  { badge: <ExitBadge status="succeeded" />, meaning: "nothing to flag" },
];

const DEVICE_LEGEND: { badge: React.ReactNode; meaning: string }[] = [
  { badge: <DeviceClassBadge deviceClass="internal" />, meaning: "lab device: BSP, driver and pinned clocks captured" },
  { badge: <DeviceClassBadge deviceClass="external" />, meaning: "retail phone or Linux box: those dimensions do not exist" },
];

/** Explains the chip vocabulary once per table, collapsed by default. */
export function StatusLegend() {
  return (
    <details className="group mt-3">
      <summary className="eyebrow inline-flex cursor-pointer list-none items-center gap-1.5 text-ink-3 hover:text-ink">
        <span className="inline-block transition-transform group-open:rotate-90">▸</span>
        What the flags mean
      </summary>
      <dl className="mt-2 grid gap-x-6 gap-y-1.5 border-l-2 border-rule pl-3 text-xs sm:grid-cols-[max-content_1fr]">
        {[...LEGEND, ...DEVICE_LEGEND].map((entry, i) => (
          <div key={i} className="contents">
            <dt className="py-px">{entry.badge}</dt>
            <dd className="py-px text-ink-2">{entry.meaning}</dd>
          </div>
        ))}
      </dl>
    </details>
  );
}

/**
 * Device class. `internal` is the default expectation (a lab device that
 * reported its BSP, driver and pinned clocks), so it stays quiet; the
 * dashed chip on `external` says the same row is missing those dimensions
 * by nature, not by accident.
 */
export function DeviceClassBadge({ deviceClass }: { deviceClass: string }) {
  if (deviceClass === "external") {
    return (
      <span
        className="eyebrow inline-flex items-center rounded-[2px] border border-dashed border-ink-3/50 bg-paper px-1.5 py-[1px] whitespace-nowrap text-ink-2"
        title="External device: a retail phone or a Linux box. It reports no BSP, SUMD driver or pinned clocks, so those columns are empty by design."
      >
        external
      </span>
    );
  }
  return (
    <span
      className="eyebrow inline-flex items-center whitespace-nowrap text-ink-3"
      title="Internal lab device: BSP, SUMD driver and pinned clocks were captured."
    >
      internal
    </span>
  );
}
