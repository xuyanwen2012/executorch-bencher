import { useState, type ReactNode } from "react";
import { Absent } from "./State";

export interface Field {
  label: string;
  /** `null`/`undefined` renders the absent marker. */
  value: ReactNode | null | undefined;
  mono?: boolean;
  /** Render the value on its own full-width line (JSON, long text). */
  block?: boolean;
  /** Explains the field on hover. */
  hint?: string;
}

export function FieldGroup({
  title,
  note,
  fields,
  children,
}: {
  title: string;
  note?: ReactNode;
  fields?: Field[];
  children?: ReactNode;
}) {
  return (
    <section className="panel flex flex-col">
      <div className="flex items-baseline gap-2 border-b border-rule bg-wash px-3 py-2">
        <h2 className="eyebrow text-ink-2">{title}</h2>
        {note ? <span className="text-xs text-ink-3">{note}</span> : null}
      </div>
      {fields ? (
        <dl className="grid grid-cols-[minmax(6rem,max-content)_minmax(0,1fr)] items-baseline gap-x-5 gap-y-2 px-3 py-3">
          {fields.map((f) => (
            <div
              key={f.label}
              className={f.block && f.value !== null && f.value !== undefined ? "col-span-2 grid gap-1" : "contents"}
            >
              <dt className="eyebrow pt-px text-ink-3" title={f.hint}>
                {f.label}
              </dt>
              <dd className={`min-w-0 break-words ${f.mono ? "font-mono text-xs" : ""}`}>
                {f.value === null || f.value === undefined || f.value === "" ? <Absent /> : f.value}
              </dd>
            </div>
          ))}
        </dl>
      ) : null}
      {children}
    </section>
  );
}

/** A hash or other long identifier: never wraps mid-row, full value on hover. */
export function Hash({ value, full = false }: { value: string; full?: boolean }) {
  return (
    <span className={`font-mono text-xs ${full ? "break-all" : "clip"}`} title={value}>
      {value}
    </span>
  );
}

/** "4 entries" / "1 key": says how much is behind a collapsed blob. */
function countLabel(value: unknown): string {
  const n = Array.isArray(value) ? value.length : Object.keys(value as object).length;
  return Array.isArray(value) ? `${n} ${n === 1 ? "entry" : "entries"}` : `${n} ${n === 1 ? "key" : "keys"}`;
}

function pretty(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2) ?? "null";
  } catch {
    return String(value);
  }
}

function isEmpty(value: unknown): boolean {
  if (value === null || value === undefined) return true;
  if (Array.isArray(value)) return value.length === 0;
  if (typeof value === "object") return Object.keys(value as object).length === 0;
  return false;
}

/**
 * A JSON blob: pretty printed, collapsed when it is long, with the number
 * of keys or entries stated up front so a reader can decide whether to open
 * it. Empty objects say so instead of showing `{}`.
 */
export function JsonValue({ value }: { value: unknown }) {
  const text = pretty(value);
  const lines = text.split("\n").length;
  const long = lines > 8;
  const [open, setOpen] = useState(!long);
  const [copied, setCopied] = useState(false);

  if (isEmpty(value)) {
    return <span className="text-ink-3 italic">{Array.isArray(value) ? "no entries" : "none recorded"}</span>;
  }

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      setCopied(false);
    }
  };

  return (
    <div className="overflow-hidden rounded-sm border border-rule">
      <div className="flex items-center gap-2 border-b border-rule bg-wash px-2 py-1">
        <button
          type="button"
          onClick={() => setOpen(!open)}
          aria-expanded={open}
          className="eyebrow inline-flex items-center gap-1.5 text-ink-3 hover:text-ink"
        >
          <span className={`inline-block transition-transform ${open ? "rotate-90" : ""}`}>▸</span>
          {countLabel(value)}
        </button>
        <span className="font-mono text-[11px] text-ink-3">{lines} lines</span>
        <button
          type="button"
          onClick={() => void copy()}
          className="eyebrow ml-auto text-ink-3 hover:text-ink"
        >
          {copied ? "Copied" : "Copy"}
        </button>
      </div>
      {open ? (
        <pre className="max-h-72 overflow-auto bg-paper px-2 py-1.5 font-mono text-[11.5px] leading-relaxed text-ink-2">
          {text}
        </pre>
      ) : null}
    </div>
  );
}
