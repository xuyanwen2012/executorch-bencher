import type { ReactNode } from "react";
import { failureOf } from "../api/errors";
import { ABSENT } from "../lib/format";

export function Loading({ label = "Loading…" }: { label?: string }) {
  return (
    <div className="panel my-4 px-4 py-10 text-center" role="status">
      <p className="eyebrow text-ink-3">{label}</p>
      <div className="mx-auto mt-3 h-px w-40 overflow-hidden bg-rule">
        <div className="h-px w-1/3 animate-pulse bg-prefill" />
      </div>
    </div>
  );
}

export function ErrorState({ error, onRetry }: { error: unknown; onRetry: () => void }) {
  const failure = failureOf(error);
  const unreachable = failure.kind === "unreachable";
  return (
    <div className="my-4 max-w-2xl rounded-sm border border-danger/30 bg-danger-soft" role="alert">
      <p className="eyebrow border-b border-danger/20 px-4 py-2 text-danger">
        {unreachable ? "Backend unreachable" : `Request failed · HTTP ${failure.status}`}
      </p>
      <div className="px-4 py-3">
        {failure.kind === "api" ? (
          <p className="text-ink">
            <span className="font-mono font-medium text-danger">{failure.code}</span>
            <span className="text-ink-2"> — {failure.message}</span>
          </p>
        ) : (
          <p className="text-ink-2">
            The dashboard got no response from the API. Check that the backend is running, then retry.
            <span className="mt-1 block font-mono text-xs text-ink-3">{failure.message}</span>
          </p>
        )}
        <button
          type="button"
          onClick={onRetry}
          className="eyebrow mt-3 rounded-sm border border-danger/40 bg-paper px-3 py-1.5 text-danger hover:bg-danger/5"
        >
          Retry
        </button>
      </div>
    </div>
  );
}

export function EmptyState({ title, hint, action }: { title: string; hint?: string; action?: ReactNode }) {
  return (
    <div className="my-4 rounded-sm border border-dashed border-rule-strong bg-paper px-6 py-12 text-center">
      <p className="font-medium text-ink">{title}</p>
      {hint ? <p className="mx-auto mt-1 max-w-md text-ink-2">{hint}</p> : null}
      {action ? <div className="mt-4">{action}</div> : null}
    </div>
  );
}

/** Explicit marker for a value the backend did not record. */
export function Absent({ label = ABSENT }: { label?: string }) {
  return (
    <span className="text-ink-3 italic" title="The backend recorded no value for this field.">
      {label}
    </span>
  );
}

/** Compact absent marker for dense table cells. */
export function AbsentDash({ title = "not recorded" }: { title?: string }) {
  return (
    <span className="text-ink-3" title={title}>
      –
    </span>
  );
}

export type Tone = "ok" | "danger" | "warn" | "dirty" | "neutral" | "prefill" | "decode";

const TONES: Record<Tone, string> = {
  ok: "border-ok/25 bg-ok-soft text-ok",
  danger: "border-danger/25 bg-danger-soft text-danger",
  warn: "border-warn/25 bg-warn-soft text-warn",
  dirty: "border-dirty/25 bg-dirty-soft text-dirty",
  neutral: "border-rule-strong/60 bg-wash text-ink-2",
  prefill: "border-prefill/25 bg-prefill-soft text-prefill",
  decode: "border-decode/25 bg-decode-soft text-decode",
};

/** Text-only variant: the expected, unremarkable value in a status column. */
const PLAIN_TONES: Record<Tone, string> = {
  ok: "text-ok",
  danger: "text-danger",
  warn: "text-warn",
  dirty: "text-dirty",
  neutral: "text-ink-3",
  prefill: "text-prefill",
  decode: "text-decode",
};

/**
 * A status chip. Filled chips are reserved for values that need a
 * reviewer's attention; `plain` renders the ordinary case as quiet text so
 * that a column of healthy runs does not compete with the numbers.
 */
export function Badge({
  tone = "neutral",
  plain = false,
  title,
  children,
}: {
  tone?: Tone;
  plain?: boolean;
  title?: string;
  children: ReactNode;
}) {
  const skin = plain
    ? `border-transparent ${PLAIN_TONES[tone]}`
    : TONES[tone];
  return (
    <span
      title={title}
      className={`eyebrow inline-flex items-center rounded-[2px] border px-1.5 py-[1px] whitespace-nowrap ${skin}`}
    >
      {children}
    </span>
  );
}

/** Turns `infrastructure_error` into `infrastructure error` for display. */
export function humanToken(token: string): string {
  return token.replace(/_/g, " ");
}
