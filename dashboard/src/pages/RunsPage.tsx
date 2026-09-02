import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { Link, useSearchParams } from "react-router";
import { api, type components } from "../api/client";
import { unwrap } from "../api/errors";
import { FilterBar, type ExtraChip, type FilterField } from "../components/FilterBar";
import { AbsentDash, EmptyState, ErrorState, Loading } from "../components/State";
import {
  CorrectnessBadge,
  DeviceClassBadge,
  DirtyBadge,
  ExitBadge,
  StatusLegend,
  ThrottledBadge,
} from "../components/Status";
import { Timestamp } from "../components/Timestamp";
import {
  CORRECTNESS_RESULTS,
  DEVICE_CLASSES,
  EXIT_STATUSES,
  PLATFORMS,
  hasAnyFilter,
  parseRunsFilters,
  runsFiltersToParams,
  runsQuery,
  type RunsFilterKey,
  type RunsFilters,
} from "../lib/filters";
import {
  abbreviateSha,
  formatCount,
  formatTokPerSec,
  modelLabel,
  pluralRuns,
  splitLocalTime,
} from "../lib/format";
import { parseModelName } from "../lib/model";

type Summary = components["schemas"]["RunSummaryResponse"];

const PAGE_SIZE = 50;

/** Key filters that arrive from a results-row link but have no control of
 * their own; they are shown as removable chips instead. */
const LINKED_ONLY: readonly { key: RunsFilterKey; label: string }[] = [
  { key: "gpu_clock_mhz", label: "GPU clock" },
  { key: "mif_clock_mhz", label: "MIF clock" },
  { key: "int_clock_mhz", label: "INT clock" },
  { key: "prompt_sha256", label: "Prompt hash" },
];

export function RunsPage() {
  const [params, setParams] = useSearchParams();
  const filters = parseRunsFilters(params);

  const models = useQuery({
    queryKey: ["models"],
    queryFn: () => unwrap(api.GET("/api/v1/models")),
  });

  const runs = useInfiniteQuery({
    queryKey: ["runs", filters],
    queryFn: ({ pageParam }) =>
      unwrap(
        api.GET("/api/v1/runs", {
          params: { query: { ...runsQuery(filters), limit: PAGE_SIZE, cursor: pageParam } },
        }),
      ),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (last) => last.next_cursor ?? undefined,
  });

  const setFilters = (next: RunsFilters) => setParams(runsFiltersToParams(next));
  const onFilter = (key: RunsFilterKey, value: string) => setFilters({ ...filters, [key]: value || undefined });

  const fields: FilterField<RunsFilterKey>[] = [
    { key: "platform", label: "Platform", options: PLATFORMS.map((v) => ({ value: v, label: v })) },
    { key: "device_class", label: "Device class", options: DEVICE_CLASSES.map((v) => ({ value: v, label: v })) },
    { key: "device_serial", label: "Host", placeholder: "serial or hostname" },
    {
      key: "model_asset_id",
      label: "Model",
      options: (models.data ?? []).map((m) => ({ value: m.id, label: modelLabel(m.original_name) })),
    },
    { key: "git_commit_sha", label: "Commit SHA", placeholder: "full SHA" },
    { key: "git_branch", label: "Branch", placeholder: "branch name" },
    {
      key: "git_dirty",
      label: "Working tree",
      options: [
        { value: "false", label: "clean only" },
        { value: "true", label: "dirty only" },
      ],
    },
    { key: "sumd_driver_version", label: "SUMD driver", placeholder: "version" },
    { key: "bsp_version", label: "BSP", placeholder: "version" },
    { key: "host_accelerator", label: "Accelerator", placeholder: "device name" },
    { key: "exit_status", label: "Exit status", options: EXIT_STATUSES.map((v) => ({ value: v, label: v.replace(/_/g, " ") })) },
    {
      key: "correctness_result",
      label: "Correctness",
      options: CORRECTNESS_RESULTS.map((v) => ({ value: v, label: v.replace(/_/g, " ") })),
    },
  ];

  const chips: ExtraChip[] = LINKED_ONLY.filter(({ key }) => filters[key] !== undefined).map(({ key, label }) => ({
    key,
    label,
    value: filters[key] as string,
    onRemove: () => setFilters({ ...filters, [key]: undefined }),
  }));

  const items: Summary[] = runs.data?.pages.flatMap((p) => p.items) ?? [];
  const complete = !runs.hasNextPage;

  return (
    <section>
      <header className="mb-4">
        <h1 className="font-cond text-[22px] leading-tight font-semibold tracking-tight text-ink">Runs</h1>
        <p className="mt-0.5 max-w-prose text-ink-2">
          Every recorded run, newest first — one row per repetition, including the ones that failed.
        </p>
      </header>

      <FilterBar
        fields={fields}
        values={filters}
        onChange={onFilter}
        onClear={() => setFilters({})}
        extraChips={chips}
        status={
          items.length > 0
            ? `${formatCount(items.length)}${complete ? "" : "+"} run${items.length === 1 ? "" : "s"}`
            : undefined
        }
      />

      {runs.isPending ? <Loading label="Loading runs…" /> : null}
      {runs.isError ? <ErrorState error={runs.error} onRetry={() => void runs.refetch()} /> : null}
      {runs.data && items.length === 0 ? (
        hasAnyFilter(filters) ? (
          <EmptyState
            title="No runs match these filters."
            hint="Widen or clear the filters above to bring runs back."
          />
        ) : (
          <EmptyState title="No runs recorded yet." hint="Runs appear here as soon as one is imported." />
        )
      ) : null}
      {items.length > 0 ? (
        <>
          <RunsTable items={items} />
          <div className="mt-3 flex flex-wrap items-center gap-3">
            {runs.hasNextPage ? (
              <button
                type="button"
                onClick={() => void runs.fetchNextPage()}
                disabled={runs.isFetchingNextPage}
                className="eyebrow rounded-sm border border-rule bg-paper px-3 py-1.5 text-ink-2 hover:border-rule-strong hover:bg-wash disabled:opacity-50"
              >
                {runs.isFetchingNextPage ? "Loading…" : `Load ${PAGE_SIZE} more`}
              </button>
            ) : null}
            <span className="text-xs text-ink-3">
              {complete
                ? `All ${pluralRuns(items.length)} matching these filters are listed.`
                : `${pluralRuns(items.length)} listed so far.`}
            </span>
          </div>
          <StatusLegend />
        </>
      ) : null}
    </section>
  );
}

function RunsTable({ items }: { items: Summary[] }) {
  const columns = 9;
  let lastDate: string | null = null;

  return (
    <div className="overflow-x-auto rounded-sm border border-rule bg-paper">
      <table className="w-full min-w-[62rem] border-collapse text-[13px]">
        <caption className="sr-only">Benchmark runs, newest first</caption>
        <thead>
          <tr className="border-b border-rule text-center">
            <th className="pt-2" colSpan={columns - 2} />
            <th className="eyebrow border-b border-rule px-2 pt-2 pb-1 text-ink-3" colSpan={2}>
              Throughput — tok/s
            </th>
          </tr>
          <tr className="border-b border-rule-strong">
            <Th className="sticky left-0 z-20 bg-paper pl-3 text-left">Started</Th>
            <Th className="text-left">Host</Th>
            <Th className="text-left">Model</Th>
            <Th className="text-left">Commit</Th>
            <Th className="text-left">Driver / accelerator</Th>
            <Th className="text-right" hint="Repetition number within the configuration">
              Rep
            </Th>
            <Th className="text-left">Status</Th>
            <Th className="text-right text-prefill">Prefill</Th>
            <Th className="pr-3 text-right text-decode">Decode</Th>
          </tr>
        </thead>
        <tbody>
          {items.map((run) => {
            const date = splitLocalTime(run.started_at)?.date ?? null;
            const newDay = date !== lastDate;
            lastDate = date;
            return (
              <RunRow
                key={run.id}
                run={run}
                dayHeader={newDay ? date : null}
                grouped={date !== null}
                columns={columns}
              />
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

function Th({ children, className = "", hint }: { children: ReactNode; className?: string; hint?: string }) {
  return (
    <th scope="col" title={hint} className={`eyebrow px-2 py-1.5 align-bottom text-ink-3 ${className}`}>
      {children}
    </th>
  );
}

/** An explicit absent marker for a throughput a run never measured. */
function NotRecorded({ hint }: { hint: string }) {
  return (
    <span className="font-sans text-[11px] whitespace-nowrap text-ink-3 italic" title={hint}>
      not recorded
    </span>
  );
}

function RunRow({
  run,
  dayHeader,
  grouped,
  columns,
}: {
  run: Summary;
  dayHeader: string | null;
  /** The table carries the date in a group heading, so the row shows the clock only. */
  grouped: boolean;
  columns: number;
}) {
  const model = parseModelName(run.model_asset.original_name);
  const decode = formatTokPerSec(run.decode_tokens_per_sec);
  const prefill = formatTokPerSec(run.prefill_tokens_per_sec);
  const hardware = run.sumd_driver_version ?? run.host_accelerator;

  return (
    <>
      {dayHeader ? (
        <tr className="border-t border-rule-strong bg-wash">
          <th
            scope="colgroup"
            colSpan={columns}
            className="eyebrow px-3 py-1 text-left font-mono text-[11px] tracking-normal text-ink-3 normal-case"
          >
            {dayHeader}
          </th>
        </tr>
      ) : null}
      <tr className="group border-t border-rule align-top hover:bg-wash">
        <td className="sticky left-0 z-10 bg-paper py-2 pr-2 pl-3 whitespace-nowrap after:absolute after:inset-y-0 after:right-0 after:w-px after:bg-rule group-hover:bg-wash">
          <Link
            to={`/runs/${run.id}`}
            className="font-mono text-ink underline decoration-rule-strong underline-offset-2 hover:decoration-prefill"
            title={`Open run ${run.id}`}
          >
            <Timestamp iso={run.started_at} timeOnly={grouped} />
          </Link>
        </td>
        <td className="px-2 py-2 whitespace-nowrap">
          <span className="block font-mono text-xs text-ink">{run.device_serial}</span>
          <span className="flex items-center gap-1.5">
            <span className="font-mono text-[11px] text-ink-3">
              {run.platform}
              {run.device_model ? ` · ${run.device_model}` : ""}
            </span>
            {run.device_class === "external" ? <DeviceClassBadge deviceClass={run.device_class} /> : null}
          </span>
        </td>
        <td className="max-w-[12rem] px-2 py-2">
          <span className="clip text-xs font-medium text-ink" title={model.label}>
            {model.identity}
          </span>
          {model.qualifiers ? (
            <span className="clip font-mono text-[11px] text-ink-3">{model.qualifiers}</span>
          ) : null}
        </td>
        <td className="px-2 py-2">
          <span className="flex items-center gap-1.5">
            <span className="font-mono text-xs text-ink" title={run.git_commit_sha}>
              {abbreviateSha(run.git_commit_sha)}
            </span>
            {run.git_dirty ? <DirtyBadge /> : null}
          </span>
          <span className="block font-mono text-[11px] text-ink-3">
            {run.git_branch ?? <AbsentDash title="No branch recorded" />}
          </span>
        </td>
        <td className="max-w-[13rem] px-2 py-2 font-mono text-xs text-ink-2">
          {hardware ? (
            <span className="clip" title={hardware}>
              {hardware}
            </span>
          ) : (
            <AbsentDash />
          )}
        </td>
        <td className="px-2 py-2 text-right font-mono text-xs text-ink-2">{run.repetition}</td>
        <td className="px-2 py-2">
          <span className="flex flex-wrap items-center gap-1">
            <ExitBadge status={run.exit_status} />
            <CorrectnessBadge result={run.correctness_result} />
            {run.thermal_throttling ? <ThrottledBadge /> : null}
          </span>
        </td>
        <td className="px-2 py-2 text-right font-mono text-prefill">
          {prefill ?? <NotRecorded hint="This run recorded no prefill throughput." />}
        </td>
        <td className="py-2 pr-3 pl-2 text-right font-mono text-decode">
          {decode ?? <NotRecorded hint="This run recorded no decode throughput; it is not zero." />}
        </td>
      </tr>
    </>
  );
}
