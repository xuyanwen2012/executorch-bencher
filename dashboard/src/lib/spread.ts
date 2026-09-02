// Geometry for the dispersion gauge drawn beside each throughput median.
//
// The gauge answers the question a benchmark reviewer asks first: "how
// much did the repetitions of this configuration disagree?" It is scaled
// to the *relative* deviation from the row's own median, not to an
// absolute tok/s axis, so a 1 GB/s phone row and a 6000 tok/s GPU row can
// be compared side by side on one page. Deviations beyond the full-scale
// bound are clamped and reported so the row can be marked as an outlier.

export interface SpreadStats {
  median: number;
  min: number;
  max: number;
  n: number;
}

export interface SpreadGeometry {
  /** Left edge of the min–max band, 0–100 across the track. */
  leftPct: number;
  /** Right edge of the min–max band, 0–100 across the track. */
  rightPct: number;
  /** Position of the median tick, 0–100 (50 unless an end is clamped). */
  medianPct: number;
  /** The min ran past the full-scale bound. */
  clampedLow: boolean;
  /** The max ran past the full-scale bound. */
  clampedHigh: boolean;
  /** Widest relative deviation from the median, as a fraction. */
  deviation: number;
  /** A single measurement: draw the tick only, no band. */
  single: boolean;
}

/** Relative deviation at which the track is full width, each way. */
export const FULL_SCALE = 0.1;

const clamp = (value: number, low: number, high: number) => Math.min(high, Math.max(low, value));

/**
 * Maps a median/min/max onto a 0–100 track centred on the median, where
 * the track spans ±`fullScale` relative deviation.
 *
 * Returns `null` when there is nothing meaningful to draw (no stats, or a
 * median of zero, which has no relative scale).
 */
export function spreadGeometry(
  stats: SpreadStats | null | undefined,
  fullScale: number = FULL_SCALE,
): SpreadGeometry | null {
  if (!stats || !Number.isFinite(stats.median) || stats.median <= 0) return null;
  const low = (stats.min - stats.median) / stats.median;
  const high = (stats.max - stats.median) / stats.median;
  if (!Number.isFinite(low) || !Number.isFinite(high)) return null;
  const toPct = (relative: number) => 50 + (clamp(relative, -fullScale, fullScale) / fullScale) * 50;
  return {
    leftPct: toPct(low),
    rightPct: toPct(high),
    medianPct: 50,
    clampedLow: low < -fullScale,
    clampedHigh: high > fullScale,
    deviation: Math.max(Math.abs(low), Math.abs(high)),
    single: stats.n <= 1 || stats.min === stats.max,
  };
}

/** Tooltip text for the gauge: the spread as a share of the median. */
export function describeSpread(stats: SpreadStats | null | undefined): string | null {
  const geometry = spreadGeometry(stats);
  if (!stats || !geometry) return null;
  if (geometry.single) return `n=${stats.n}: every measurement equal`;
  const pct = (value: number) => {
    const relative = ((value - stats.median) / stats.median) * 100;
    return `${relative >= 0 ? "+" : "−"}${Math.abs(relative).toFixed(1)}%`;
  };
  return `n=${stats.n}: min ${pct(stats.min)}, max ${pct(stats.max)} of the median`;
}
