import { useQuery } from "@tanstack/react-query";
import { Link, useParams } from "react-router";
import { api } from "../api/client";
import { isNotFound, unwrap } from "../api/errors";
import { ArtifactCard, MissingArtifacts, type ArtifactSlot } from "../components/ArtifactCard";
import { Absent, EmptyState, ErrorState, Loading } from "../components/State";
import {
  CorrectnessBadge,
  DeviceClassBadge,
  DirtyBadge,
  ExitBadge,
  ThrottledBadge,
} from "../components/Status";
import { Timestamp } from "../components/Timestamp";
import { runsFiltersToParams } from "../lib/filters";
import { abbreviateSha, formatTokPerSec } from "../lib/format";
import { parseModelName } from "../lib/model";
import { BuildIdentityGroup } from "./run-detail/BuildIdentityGroup";
import { HostGroup } from "./run-detail/HostGroup";
import { ModelGroup } from "./run-detail/ModelGroup";
import { ResultsGroup } from "./run-detail/ResultsGroup";
import { RunMetadataGroup } from "./run-detail/RunMetadataGroup";
import type { Run } from "./run-detail/shared";

export function RunDetailPage() {
  const { id = "" } = useParams();
  const query = useQuery({
    queryKey: ["run", id],
    queryFn: () => unwrap(api.GET("/api/v1/runs/{id}", { params: { path: { id } } })),
    // An empty id can never resolve; the page says so below instead of
    // leaving a disabled query pending forever behind "Loading run…".
    enabled: id !== "",
  });

  if (id === "") return <RunNotFound id={id} />;
  if (query.isPending) return <Loading label="Loading run…" />;
  if (query.isError) {
    if (isNotFound(query.error)) return <RunNotFound id={id} />;
    return (
      <div>
        <ErrorState error={query.error} onRetry={() => void query.refetch()} />
        <p className="text-xs">
          <Link to="/runs" className="text-ink-2 underline decoration-rule-strong underline-offset-2 hover:text-ink">
            Back to all runs
          </Link>
        </p>
      </div>
    );
  }
  return <RunDetail run={query.data} />;
}

function RunNotFound({ id }: { id: string }) {
  return (
    <EmptyState
      title="Run not found."
      hint={id === "" ? "No run ID was given." : `No run is recorded under the ID ${id}.`}
      action={
        <Link
          to="/runs"
          className="eyebrow rounded-sm border border-rule bg-paper px-3 py-1.5 text-ink-2 hover:border-rule-strong hover:bg-wash"
        >
          Back to all runs
        </Link>
      }
    />
  );
}

function RunDetail({ run }: { run: Run }) {
  const model = parseModelName(run.model_asset?.original_name ?? "");

  const slots: ArtifactSlot[] = [
    { slot: "input prompt", artifact: run.input_artifact },
    { slot: "output", artifact: run.output_artifact },
    { slot: "stdout", artifact: run.stdout_artifact },
    { slot: "stderr", artifact: run.stderr_artifact },
    { slot: "crash log", artifact: run.crash_artifact },
  ];
  const attached = slots.filter((s) => s.artifact);
  const missing = slots.filter((s) => !s.artifact).map((s) => s.slot);

  // Sibling runs: same configuration, so a reader can jump straight to the
  // other repetitions this run should be compared against.
  const siblings = `/runs?${runsFiltersToParams({
    platform: run.platform,
    device_serial: run.device_serial,
    model_asset_id: run.model_asset?.id,
    git_commit_sha: run.git_commit_sha,
    git_dirty: String(run.git_dirty),
  }).toString()}`;

  return (
    <section className="space-y-4">
      <nav className="flex items-center gap-1.5 text-xs text-ink-3" aria-label="Breadcrumb">
        <Link to="/runs" className="underline decoration-rule-strong underline-offset-2 hover:text-ink">
          Runs
        </Link>
        <span aria-hidden>/</span>
        <span className="font-mono text-ink-2">{run.id}</span>
      </nav>

      {/* Headline: what this run measured, and whether it can be trusted. */}
      <header className="panel overflow-hidden">
        <div className="flex flex-wrap items-start justify-between gap-x-6 gap-y-3 px-4 py-3">
          <div className="min-w-0">
            <p className="eyebrow text-ink-3">
              {run.platform} · repetition {run.repetition}
            </p>
            <h1 className="mt-1 font-cond text-[24px] leading-tight font-semibold tracking-tight text-ink">
              {model.identity || "Run"}
              {model.qualifiers ? (
                <span className="ml-2 font-mono text-[13px] font-normal text-ink-3">{model.qualifiers}</span>
              ) : null}
            </h1>
            <p className="mt-1 flex flex-wrap items-center gap-x-2 gap-y-1 text-ink-2">
              <span className="font-mono text-xs">{run.device_serial}</span>
              {run.device_model ? (
                <span className="font-mono text-xs text-ink-3">({run.device_model})</span>
              ) : null}
              <DeviceClassBadge deviceClass={run.device_class} />
              <span className="text-rule-strong">·</span>
              <span className="font-mono text-xs" title={run.git_commit_sha}>
                {abbreviateSha(run.git_commit_sha)}
              </span>
              <span className="text-rule-strong">·</span>
              <Timestamp iso={run.started_at} />
            </p>
          </div>
          <div className="flex flex-wrap items-center gap-1.5">
            <ExitBadge status={run.exit_status} />
            <CorrectnessBadge result={run.correctness_result} />
            {run.thermal_throttling ? <ThrottledBadge /> : null}
            {run.git_dirty ? <DirtyBadge /> : null}
          </div>
        </div>

        <div className="grid grid-cols-2 gap-px border-t border-rule bg-rule sm:grid-cols-4">
          <Reading label="Prefill" unit="tok/s" tone="prefill" value={formatTokPerSec(run.prefill_tokens_per_sec)} />
          <Reading label="Decode" unit="tok/s" tone="decode" value={formatTokPerSec(run.decode_tokens_per_sec)} />
          <Reading label="Input" unit="tokens" value={run.input_token_count.toLocaleString()} />
          <Reading label="Output" unit="tokens" value={run.output_token_count.toLocaleString()} />
        </div>

        {run.error_summary ? (
          <p className="border-t border-danger/25 bg-danger-soft px-4 py-2 text-danger">
            <span className="eyebrow mr-2">Error</span>
            <span className="font-mono text-xs">{run.error_summary}</span>
          </p>
        ) : null}
      </header>

      <div className="lg:columns-2 lg:gap-4 [&>section]:mb-4 [&>section]:break-inside-avoid">
        <ResultsGroup run={run} />
        <BuildIdentityGroup run={run} />
        <HostGroup run={run} />
        <ModelGroup run={run} />
        <RunMetadataGroup run={run} />
      </div>

      <section>
        <div className="mb-2 flex flex-wrap items-baseline gap-x-3">
          <h2 className="eyebrow text-ink-2">Artifacts</h2>
          <span className="text-xs text-ink-3">
            {attached.length === 0
              ? "This run references no artifacts."
              : `${attached.length} attached to this run.`}
          </span>
        </div>
        {attached.length > 0 ? (
          <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
            {attached.map((s) => (
              <ArtifactCard key={s.slot} slot={s.slot} artifact={s.artifact} />
            ))}
          </div>
        ) : null}
        <MissingArtifacts slots={missing} />
      </section>

      <p className="text-xs text-ink-3">
        <Link to={siblings} className="underline decoration-rule-strong underline-offset-2 hover:text-ink">
          See the other runs of this configuration →
        </Link>
      </p>
    </section>
  );
}

function Reading({
  label,
  unit,
  value,
  tone,
}: {
  label: string;
  unit: string;
  value: string | null;
  tone?: "prefill" | "decode";
}) {
  const colour = tone === "prefill" ? "text-prefill" : tone === "decode" ? "text-decode" : "text-ink";
  return (
    <div className="bg-paper px-4 py-3">
      <p className="eyebrow text-ink-3">{label}</p>
      {value ? (
        <p className="mt-0.5 flex items-baseline gap-1.5">
          <span className={`font-mono text-[26px] leading-none font-medium ${colour}`}>{value}</span>
          <span className="text-xs text-ink-3">{unit}</span>
        </p>
      ) : (
        <p className="mt-1.5">
          <Absent />
        </p>
      )}
    </div>
  );
}
