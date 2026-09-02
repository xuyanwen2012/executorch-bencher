import type { ReactNode } from "react";
import type { components } from "../../api/client";
import { Absent } from "../../components/State";

export type Run = components["schemas"]["RunResponse"];

export const yesNo = (v: boolean | null | undefined) => (v === null || v === undefined ? null : v ? "yes" : "no");
export const celsius = (v: number | null | undefined) => (v === null || v === undefined ? null : `${v.toFixed(1)} °C`);
export const mhz = (v: number | null | undefined) => (v === null || v === undefined ? null : `${v} MHz`);

/** On an external device the lab-only dimensions were never collectable,
 * which is a different statement from "the collector missed them". */
export function labValue(external: boolean) {
  return (value: ReactNode | null | undefined): ReactNode | null =>
    value === null || value === undefined ? (external ? <Absent label="not applicable" /> : null) : value;
}

export function withUnit(value: string | null, unit: string, tone: "prefill" | "decode"): ReactNode {
  if (!value) return null;
  const colour = tone === "prefill" ? "text-prefill" : "text-decode";
  return (
    <span className="flex items-baseline gap-1">
      <span className={`font-mono font-medium ${colour}`}>{value}</span>
      <span className="text-xs text-ink-3">{unit}</span>
    </span>
  );
}
