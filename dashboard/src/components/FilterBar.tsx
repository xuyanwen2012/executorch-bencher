import { useEffect, useRef, useState, type ReactNode } from "react";
import { FOCUS_RING } from "./focus";

export interface FilterField<K extends string> {
  key: K;
  label: string;
  /** Present for a select; absent for a free-text input. */
  options?: { value: string; label: string }[];
  placeholder?: string;
}

/** An active filter the page carries but has no control for (it arrived in
 * the URL from a results-row link), rendered as a removable chip. */
export interface ExtraChip {
  key: string;
  label: string;
  value: string;
  onRemove: () => void;
}

interface Props<K extends string> {
  fields: readonly FilterField<K>[];
  values: Partial<Record<K, string>>;
  /** Called with the committed value: at once for a select or a chip, and
   * after `debounceMs` of quiet (or Enter / blur) for a text input. */
  onChange: (key: K, value: string) => void;
  onClear?: () => void;
  extraChips?: readonly ExtraChip[];
  /** Rendered on the filter bar's top rail, e.g. a result count. */
  status?: ReactNode;
  /** Quiet time before a text edit is committed. */
  debounceMs?: number;
}

/** Default quiet time before a typed filter reaches the URL and the query. */
export const FILTER_DEBOUNCE_MS = 250;

const CONTROL = `w-full rounded-sm border bg-paper px-2 py-1.5 text-[13px] text-ink transition-colors focus:border-prefill ${FOCUS_RING}`;

/**
 * Text inputs are edited locally and committed after a pause, so a filter
 * typed character by character produces one URL entry and one request
 * rather than one per keystroke. Selects and chips commit immediately.
 */
export function FilterBar<K extends string>({
  fields,
  values,
  onChange,
  onClear,
  extraChips = [],
  status,
  debounceMs = FILTER_DEBOUNCE_MS,
}: Props<K>) {
  // A draft exists only while a text input is ahead of the committed value.
  const [drafts, setDrafts] = useState<Partial<Record<K, string>>>({});
  const timers = useRef(new Map<K, ReturnType<typeof setTimeout>>());
  const latestOnChange = useRef(onChange);
  latestOnChange.current = onChange;

  const cancel = (key: K) => {
    const handle = timers.current.get(key);
    if (handle !== undefined) clearTimeout(handle);
    timers.current.delete(key);
  };
  const cancelAll = () => {
    for (const handle of timers.current.values()) clearTimeout(handle);
    timers.current.clear();
  };
  useEffect(() => cancelAll, []);

  // Once the committed value catches up with a draft the draft is done;
  // this also keeps the input honest when the URL changes underneath it.
  useEffect(() => {
    setDrafts((current) => {
      let next: Partial<Record<K, string>> | undefined;
      for (const key of Object.keys(current) as K[]) {
        if (timers.current.has(key)) continue;
        if ((values[key] ?? "") === current[key]) {
          next ??= { ...current };
          delete next[key];
        }
      }
      return next ?? current;
    });
  }, [values]);

  const commit = (key: K, value: string) => {
    cancel(key);
    latestOnChange.current(key, value);
  };
  const edit = (key: K, value: string) => {
    setDrafts((current) => ({ ...current, [key]: value }));
    cancel(key);
    timers.current.set(
      key,
      setTimeout(() => {
        timers.current.delete(key);
        latestOnChange.current(key, value);
      }, debounceMs),
    );
  };
  const flush = (key: K) => {
    if (!timers.current.has(key)) return;
    commit(key, drafts[key] ?? "");
  };
  const clearAll = () => {
    cancelAll();
    setDrafts({});
    onClear?.();
  };

  const shown = (key: K) => drafts[key] ?? values[key] ?? "";
  const activeFields = fields.filter((f) => (values[f.key] ?? "") !== "");
  const activeCount = activeFields.length + extraChips.length;

  return (
    <section className="panel mb-4" aria-label="Filters">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-rule px-3 py-2">
        <h2 className="eyebrow text-ink-3">Filters</h2>
        {activeCount > 0 ? (
          <span className="eyebrow rounded-[2px] bg-prefill-soft px-1.5 py-px text-prefill">
            {activeCount} active
          </span>
        ) : (
          <span className="text-xs text-ink-3">showing everything</span>
        )}
        <div className="ml-auto flex items-center gap-3">
          {/* Always in the tree so the live region exists before it first
              announces; screen readers ignore regions that appear populated. */}
          <span className="text-xs text-ink-2" aria-live="polite" data-testid="filter-status">
            {status}
          </span>
          {onClear ? (
            <button
              type="button"
              onClick={clearAll}
              disabled={activeCount === 0}
              className={`eyebrow rounded-sm border border-rule px-2 py-1 text-ink-2 hover:border-rule-strong hover:bg-wash disabled:pointer-events-none disabled:opacity-35 ${FOCUS_RING}`}
            >
              Clear all
            </button>
          ) : null}
        </div>
      </div>

      <div className="grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-x-3 gap-y-2.5 px-3 py-3">
        {fields.map((field) => {
          const value = shown(field.key);
          const active = value !== "";
          const id = `filter-${field.key}`;
          const border = active ? "border-prefill/60 bg-prefill-soft/40" : "border-rule";
          return (
            <div key={field.key} className="min-w-0">
              <label htmlFor={id} className="eyebrow mb-1 flex items-center gap-1 text-ink-3">
                {active ? <span className="h-1.5 w-1.5 rounded-full bg-prefill" aria-hidden /> : null}
                <span className={active ? "text-ink" : undefined}>{field.label}</span>
              </label>
              {field.options ? (
                <select
                  id={id}
                  value={value}
                  onChange={(e) => commit(field.key, e.target.value)}
                  className={`${CONTROL} ${border}`}
                >
                  <option value="">Any</option>
                  {/* A value that arrived in the URL but matches nothing in the
                      current facets still has to show, or the control would
                      silently read "Any" while the filter is in force. */}
                  {active && !field.options.some((o) => o.value === value) ? (
                    <option value={value}>{value} (no matches)</option>
                  ) : null}
                  {field.options.map((o) => (
                    <option key={o.value} value={o.value}>
                      {o.label}
                    </option>
                  ))}
                </select>
              ) : (
                <input
                  id={id}
                  value={value}
                  placeholder={field.placeholder ?? "Any"}
                  onChange={(e) => edit(field.key, e.target.value)}
                  onBlur={() => flush(field.key)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") flush(field.key);
                  }}
                  className={`${CONTROL} ${border} font-mono placeholder:font-sans placeholder:text-ink-3`}
                />
              )}
            </div>
          );
        })}
      </div>

      {activeCount > 0 ? (
        <div className="flex flex-wrap items-center gap-1.5 border-t border-rule bg-wash px-3 py-2">
          <span className="eyebrow mr-1 text-ink-3">Active</span>
          {activeFields.map((field) => (
            <Chip
              key={field.key}
              label={field.label}
              value={labelFor(field, values[field.key] ?? "")}
              onRemove={() => commit(field.key, "")}
            />
          ))}
          {extraChips.map((chip) => (
            <Chip key={chip.key} label={chip.label} value={chip.value} onRemove={chip.onRemove} />
          ))}
        </div>
      ) : null}
    </section>
  );
}

function labelFor<K extends string>(field: FilterField<K>, value: string): string {
  return field.options?.find((o) => o.value === value)?.label ?? value;
}

function Chip({ label, value, onRemove }: { label: string; value: string; onRemove: () => void }) {
  return (
    <span
      className="inline-flex max-w-[22rem] items-center gap-1 rounded-[2px] border border-rule bg-paper py-px pr-1 pl-1.5 text-xs"
      title={`${label}: ${value}`}
    >
      <span className="eyebrow whitespace-nowrap text-ink-3">{label}</span>
      <span className="clip font-mono text-ink">{value}</span>
      <button
        type="button"
        onClick={onRemove}
        aria-label={`Remove the ${label} filter`}
        className={`rounded-[2px] px-1 leading-none text-ink-3 hover:bg-danger-soft hover:text-danger ${FOCUS_RING}`}
      >
        ×
      </button>
    </span>
  );
}
