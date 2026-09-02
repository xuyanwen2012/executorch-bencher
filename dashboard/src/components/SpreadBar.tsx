import { FULL_SCALE, describeSpread, spreadGeometry, type SpreadStats } from "../lib/spread";

// The dispersion gauge. A 56px track centred on the row's own median,
// spanning ±10% relative deviation, with the min–max band drawn on it.
// A band that fills the track is a configuration whose repetitions
// disagreed by more than a tenth — usually thermal throttling or a cold
// first run — and the arrowhead says the real spread is wider still.

export function SpreadBar({ stats, tone }: { stats: SpreadStats | null | undefined; tone: "prefill" | "decode" }) {
  const geometry = spreadGeometry(stats);
  if (!geometry) return null;
  const colour = tone === "prefill" ? "bg-prefill" : "bg-decode";
  const label = describeSpread(stats) ?? "";
  const wide = geometry.deviation >= FULL_SCALE;

  return (
    <span
      className="relative block h-2.5 w-full shrink-0"
      title={`Spread of the repetitions around the median (full width = ±${(FULL_SCALE * 100).toFixed(0)}%). ${label}`}
      aria-label={label}
      role="img"
    >
      {/* Axis: a hairline the full width of the ±10% window. */}
      <span className="absolute inset-x-0 top-1/2 h-px -translate-y-1/2 bg-rule-strong/70" />
      {geometry.single ? (
        <span className={`absolute top-1/2 left-1/2 h-2 w-px -translate-x-1/2 -translate-y-1/2 ${colour}`} />
      ) : (
        <>
          <span
            className={`absolute top-1/2 h-[4px] min-w-[2px] -translate-y-1/2 rounded-[1px] ${colour} ${wide ? "opacity-100" : "opacity-40"}`}
            style={{ left: `${geometry.leftPct}%`, right: `${100 - geometry.rightPct}%` }}
          />
          <span className={`absolute top-1/2 left-1/2 h-2.5 w-[1.5px] -translate-x-1/2 -translate-y-1/2 ${colour}`} />
          {geometry.clampedLow ? <Arrow side="left" colour={colour} /> : null}
          {geometry.clampedHigh ? <Arrow side="right" colour={colour} /> : null}
        </>
      )}
    </span>
  );
}

function Arrow({ side, colour }: { side: "left" | "right"; colour: string }) {
  return (
    <span
      className={`absolute top-1/2 h-[5px] w-[5px] -translate-y-1/2 rotate-45 ${colour} ${
        side === "left" ? "left-0" : "right-0"
      }`}
    />
  );
}

/** One line explaining the gauge, shown under the results table. */
export function SpreadLegend({ tone }: { tone: "prefill" | "decode" }) {
  return (
    <p className="mt-3 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-ink-3">
      <span className="eyebrow">Spread gauge</span>
      <span className="inline-block w-14 align-middle">
        <SpreadBar stats={{ median: 100, min: 97, max: 103, n: 3 }} tone={tone} />
      </span>
      <span>
        min–max of the repetitions around the median; full width is ±{(FULL_SCALE * 100).toFixed(0)}%, and a
        diamond at either end means the spread runs past it.
      </span>
    </p>
  );
}
