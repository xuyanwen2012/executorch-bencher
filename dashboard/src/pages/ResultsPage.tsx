import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { Link, useSearchParams } from "react-router";
import { api, type components } from "../api/client";
import { unwrap } from "../api/errors";
import { FilterBar, type FilterField } from "../components/FilterBar";
import { FOCUS_RING } from "../components/focus";
import { SpreadBar, SpreadLegend } from "../components/SpreadBar";
import { Absent, AbsentDash, Badge, EmptyState, ErrorState, Loading } from "../components/State";
import { DirtyBadge, StatusLegend, ThrottledBadge } from "../components/Status";
import { Th } from "../components/Table";
import { Timestamp } from "../components/Timestamp";
import { collapseColumns } from "../lib/collapse";
import {
  hasAnyFilter,
  parseResultsFilters,
  resultsFiltersToParams,
  resultsQuery,
  runsLinkForConfiguration,
  type ResultsFilterKey,
  type ResultsFilters,
} from "../lib/filters";
import { abbreviateSha, formatCount, formatRange, modelLabel, pluralRuns } from "../lib/format";
import { ABSENT_GLYPH, KEY_COLUMNS, type ResultColumn, type ResultRow, renderCell } from "../lib/resultsColumns";

type Row = ResultRow;
type Metric = "prefill" | "decode";

export function ResultsPage() {
  const [params, setParams] = useSearchParams();
  const filters = parseResultsFilters(params);
  const metric: Metric = params.get("metric") === "decode" ? "decode" : "prefill";
  const showAll = params.get("all") === "1";

  const query = useQuery({
    queryKey: ["results", filters],
    queryFn: () => unwrap(api.GET("/api/v1/results", { params: { query: resultsQuery(filters) } })),
    // A filter edit keeps the last table on screen, dimmed, until the new
    // one arrives, instead of flashing through the loading state.
    placeholderData: keepPreviousData,
  });
  const updating = query.isPlaceholderData || (query.isFetching && query.data !== undefined);

  // Filter and view edits replace the history entry: the back button should
  // leave the page, not undo one keystroke or toggle at a time.
  const update = (next: ResultsFilters, extra?: { metric?: Metric; all?: boolean }) => {
    const p = resultsFiltersToParams(next);
    const m = extra?.metric ?? metric;
    if (m !== "prefill") p.set("metric", m);
    if (extra?.all ?? showAll) p.set("all", "1");
    setParams(p, { replace: true });
  };
  const onFilter = (key: ResultsFilterKey, value: string) => update({ ...filters, [key]: value || undefined });

  const facets = query.data?.facets;
  const fields: FilterField<ResultsFilterKey>[] = [
    { key: "platform", label: "Platform", options: (facets?.platforms ?? []).map((v) => ({ value: v, label: v })) },
    {
      key: "device_class",
      label: "Device class",
      options: (facets?.device_classes ?? []).map((v) => ({ value: v, label: v })),
    },
    { key: "device_serial", label: "Host", options: (facets?.device_serials ?? []).map((v) => ({ value: v, label: v })) },
    {
      key: "model_asset_id",
      label: "Model",
      options: (facets?.models ?? []).map((m) => ({ value: m.id, label: modelLabel(m.original_name) })),
    },
    { key: "git_branch", label: "Branch", options: (facets?.git_branches ?? []).map((v) => ({ value: v, label: v })) },
    {
      key: "sumd_driver_version",
      label: "SUMD driver",
      options: (facets?.sumd_driver_versions ?? []).map((v) => ({ value: v, label: v })),
    },
    { key: "bsp_version", label: "BSP", options: (facets?.bsp_versions ?? []).map((v) => ({ value: v, label: v })) },
    {
      key: "host_accelerator",
      label: "Accelerator",
      options: (facets?.host_accelerators ?? []).map((v) => ({ value: v, label: v })),
    },
    {
      key: "git_dirty",
      label: "Working tree",
      options: [
        { value: "false", label: "clean only" },
        { value: "true", label: "dirty only" },
      ],
    },
  ];

  const rows = query.data?.rows;

  return (
    <section>
      <header className="mb-4 flex flex-wrap items-end justify-between gap-x-6 gap-y-3">
        <div>
          <h1 className="font-cond text-[22px] leading-tight font-semibold tracking-tight text-ink">Results</h1>
          <p className="mt-0.5 max-w-prose text-ink-2">
            One row per benchmark configuration, newest commit first. Throughput is the median over the
            configuration&rsquo;s succeeded runs, with the min–max spread beside it.
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-x-5 gap-y-2">
          <MetricToggle metric={metric} onChange={(m) => update(filters, { metric: m })} />
          <label className="flex cursor-pointer items-center gap-2 text-ink-2">
            <input
              type="checkbox"
              checked={showAll}
              onChange={(e) => update(filters, { all: e.target.checked })}
              className={`accent-prefill ${FOCUS_RING}`}
            />
            <span className="eyebrow">Show all columns</span>
          </label>
        </div>
      </header>

      <FilterBar
        fields={fields}
        values={filters}
        onChange={onFilter}
        onClear={() => update({})}
        status={rows ? `${formatCount(rows.length)} configuration${rows.length === 1 ? "" : "s"}` : undefined}
      />

      {query.isPending ? <Loading label="Loading results…" /> : null}
      {query.isError ? <ErrorState error={query.error} onRetry={() => void query.refetch()} /> : null}
      {query.data ? (
        <div className={`transition-opacity ${updating ? "opacity-60" : ""}`} aria-busy={updating}>
          <ResultsBody data={query.data} metric={metric} showAll={showAll} filtered={hasAnyFilter(filters)} />
        </div>
      ) : null}
    </section>
  );
}

function MetricToggle({ metric, onChange }: { metric: Metric; onChange: (m: Metric) => void }) {
  const option = (value: Metric, label: string) => {
    const on = metric === value;
    const tone = value === "prefill" ? "bg-prefill-soft text-prefill" : "bg-decode-soft text-decode";
    return (
      <button
        key={value}
        type="button"
        aria-pressed={on}
        onClick={() => onChange(value)}
        className={`eyebrow px-2.5 py-1.5 transition-colors ${on ? tone : "text-ink-3 hover:bg-wash hover:text-ink-2"} ${FOCUS_RING}`}
      >
        {label}
      </button>
    );
  };
  return (
    <div className="flex items-center gap-2">
      <span className="eyebrow text-ink-3">Emphasize</span>
      <div className="flex divide-x divide-rule overflow-hidden rounded-sm border border-rule bg-paper" role="group">
        {option("prefill", "Prefill")}
        {option("decode", "Decode")}
      </div>
    </div>
  );
}

function ResultsBody({
  data,
  metric,
  showAll,
  filtered,
}: {
  data: components["schemas"]["ResultsResponse"];
  metric: Metric;
  showAll: boolean;
  filtered: boolean;
}) {
  const { rows, truncated, facets } = data;
  if (rows.length === 0) {
    return facets.device_serials.length === 0 && !filtered ? (
      <EmptyState
        title="No runs recorded yet."
        hint="Import a benchmark log or insert a run, and its configuration appears here."
      />
    ) : (
      <EmptyState
        title="No configurations match these filters."
        hint="Widen or clear the filters above to bring rows back."
      />
    );
  }
  const collapsed = collapseColumns(rows, KEY_COLUMNS);
  const visibleKeys = showAll ? KEY_COLUMNS.map((c) => c.key) : collapsed.varying;
  const visible = KEY_COLUMNS.filter((c) => visibleKeys.includes(c.key));

  return (
    <>
      {truncated ? (
        <p
          className="mb-3 rounded-sm border border-warn/30 bg-warn-soft px-3 py-2 text-warn"
          role="status"
        >
          <span className="eyebrow mr-2">Truncated</span>
          Only the first {formatCount(rows.length)} configurations are listed. Narrow the filters to see the rest.
        </p>
      ) : null}

      {!showAll && collapsed.shared.length > 0 ? (
        <div
          className="mb-3 flex flex-wrap items-baseline gap-x-6 gap-y-1 rounded-sm border border-rule border-l-2 border-l-prefill bg-paper px-3 py-2"
          data-testid="shared-configuration"
        >
          <span className="eyebrow text-ink-3" title="These columns are hidden because every row shares one value.">
            Shared configuration
          </span>
          {collapsed.shared.map((s) => (
            <span key={s.key} className="flex items-baseline gap-1.5">
              <span className="eyebrow text-ink-3">{s.label}</span>
              <span className="font-mono text-xs text-ink">
                {s.value === ABSENT_GLYPH ? <AbsentDash title="Not applicable on this platform" /> : s.value}
              </span>
            </span>
          ))}
        </div>
      ) : null}

      <div className="overflow-x-auto rounded-sm border border-rule bg-paper">
        <table className="w-full min-w-[64rem] border-collapse text-[13px]">
          <caption className="sr-only">
            Benchmark configurations with median prefill and decode throughput in tokens per second
          </caption>
          <thead>
            <tr className="border-b border-rule text-center">
              <th className="pt-2" />
              <th className="eyebrow border-b border-rule px-2 pt-2 pb-1 text-left text-ink-3" colSpan={visible.length}>
                Configuration
              </th>
              <th className="pt-2" />
              <th
                className="eyebrow border-b border-rule px-2 pt-2 pb-1 text-ink-3"
                colSpan={2}
                title="Median over the configuration's succeeded runs, in tokens per second"
              >
                Median tok/s
              </th>
              <th className="pt-2" colSpan={2} />
            </tr>
            <tr className="border-b border-rule-strong">
              <Th className="sticky left-0 z-20 bg-paper pl-3 text-left">Commit</Th>
              {visible.map((c) => (
                <Th key={c.key} className={c.align === "right" ? "text-right" : "text-left"}>
                  {c.label}
                </Th>
              ))}
              <Th className="text-right" hint="Number of succeeded runs behind the median">
                n
              </Th>
              <Th className={`text-right text-prefill ${metric === "prefill" ? "bg-prefill-soft" : ""}`}>Prefill</Th>
              <Th className={`text-right text-decode ${metric === "decode" ? "bg-decode-soft" : ""}`}>Decode</Th>
              <Th className="text-left">Flags</Th>
              <Th className="pr-3 text-left">Latest run</Th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row, i) => (
              <ResultRowView
                key={rowKey(row)}
                row={row}
                visible={visible}
                metric={metric}
                newCommit={i > 0 && rows[i - 1]?.git_commit_sha !== row.git_commit_sha}
              />
            ))}
          </tbody>
        </table>
      </div>
      <SpreadLegend tone={metric} />
      <StatusLegend />
    </>
  );
}

function rowKey(r: Row): string {
  return [
    r.platform,
    r.device_serial,
    r.model_asset.id,
    r.git_commit_sha,
    r.git_dirty,
    r.sumd_driver_version,
    r.bsp_version,
    r.gpu_clock_mhz,
    r.mif_clock_mhz,
    r.int_clock_mhz,
    r.host_accelerator,
    r.prompt_sha256,
  ].join("|");
}

function ResultRowView({
  row,
  visible,
  metric,
  newCommit,
}: {
  row: Row;
  visible: readonly ResultColumn[];
  metric: Metric;
  newCommit: boolean;
}) {
  const runsLink = runsLinkForConfiguration(row);
  const n = row.prefill?.n ?? row.decode?.n ?? 0;
  return (
    <tr
      className={`group align-top hover:bg-wash ${newCommit ? "border-t-2 border-t-rule-strong" : "border-t border-rule"}`}
    >
      <td className="sticky left-0 z-10 w-[11rem] max-w-[11rem] bg-paper py-2 pr-2 pl-3 after:absolute after:inset-y-0 after:right-0 after:w-px after:bg-rule group-hover:bg-wash">
        <span className="flex items-center gap-1.5">
          <Link
            to={runsLink}
            className="font-mono text-[13px] font-medium text-ink underline decoration-rule-strong decoration-1 underline-offset-2 hover:decoration-prefill"
            title={row.git_commit_sha}
          >
            {abbreviateSha(row.git_commit_sha)}
          </Link>
          {row.git_dirty ? <DirtyBadge /> : null}
        </span>
        <span className="clip text-xs text-ink-2" title={row.git_commit_subject ?? undefined}>
          {row.git_branch ? <span className="font-mono text-ink-3">{row.git_branch}</span> : null}
          {row.git_branch && row.git_commit_subject ? <span className="text-ink-3"> · </span> : null}
          {row.git_commit_subject}
        </span>
        <span className="block text-[11px] text-ink-3">
          {row.git_commit_timestamp ? (
            <Timestamp iso={row.git_commit_timestamp} />
          ) : (
            <span title="No commit timestamp was recorded; this row is ordered by its first run.">
              first run <Timestamp iso={row.first_run_at} />
            </span>
          )}
        </span>
      </td>

      {visible.map((c) => (
        <td
          key={c.key}
          className={`px-2 py-2 ${c.width ?? ""} ${c.align === "right" ? "text-right" : ""} font-mono text-xs text-ink-2`}
        >
          {renderCell(c, row)}
        </td>
      ))}

      <td className="px-2 py-2 text-right font-mono text-xs">
        {n === 0 ? <span className="text-danger">0</span> : <span className="text-ink-2">{n}</span>}
      </td>
      <MetricCell stats={row.prefill} tone="prefill" emphasized={metric === "prefill"} baselineN={n} />
      <MetricCell stats={row.decode} tone="decode" emphasized={metric === "decode"} baselineN={n} />

      <td className="max-w-[5.5rem] px-2 py-2">
        <span className="flex flex-wrap gap-1">
          {row.not_succeeded > 0 ? (
            <Badge tone="danger" title={`${row.not_succeeded} of ${row.total_runs} runs did not succeed`}>
              {row.not_succeeded} failed
            </Badge>
          ) : null}
          {row.correctness_failed > 0 ? (
            <Badge tone="danger" title={`${row.correctness_failed} runs produced an incorrect result`}>
              {row.correctness_failed} incorrect
            </Badge>
          ) : null}
          {row.throttled > 0 ? <ThrottledBadge /> : null}
        </span>
      </td>

      <td className="py-2 pr-3 pl-2 text-[11px] whitespace-nowrap text-ink-2">
        <Timestamp iso={row.last_run_at} stacked />
        <Link
          to={runsLink}
          className="mt-0.5 block text-ink-3 underline decoration-rule-strong underline-offset-2 hover:text-prefill hover:decoration-prefill"
          title="List exactly the runs behind this row"
        >
          {pluralRuns(row.total_runs)} →
        </Link>
      </td>
    </tr>
  );
}

function MetricCell({
  stats,
  tone,
  emphasized,
  baselineN,
}: {
  stats: Row["prefill"];
  tone: Metric;
  emphasized: boolean;
  baselineN: number;
}) {
  const colour = tone === "prefill" ? "text-prefill" : "text-decode";
  const tint = emphasized ? (tone === "prefill" ? "bg-prefill-soft/50" : "bg-decode-soft/50") : "";
  const range = formatRange(stats, baselineN);
  return (
    <td className={`px-3 py-2 text-right ${tint} ${emphasized ? "min-w-[7.5rem]" : ""}`}>
      {stats ? (
        <>
          <span
            className={
              emphasized
                ? "grid grid-cols-[3rem_max-content] items-center justify-end gap-2"
                : "block"
            }
          >
            {emphasized ? <SpreadBar stats={stats} tone={tone} /> : null}
            <span className={`font-mono ${colour} ${emphasized ? "text-[15px] font-medium" : "text-[13px]"}`}>
              {stats.median.toFixed(1)}
            </span>
          </span>
          <span className="block font-mono text-[11px] whitespace-nowrap text-ink-3">{range}</span>
        </>
      ) : (
        <Absent className="block text-[11px] whitespace-nowrap" title="No succeeded run recorded this phase." />
      )}
    </td>
  );
}
