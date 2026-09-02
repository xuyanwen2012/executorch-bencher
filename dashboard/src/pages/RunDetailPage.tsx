import { useQuery } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { Link, useParams } from "react-router";
import { api, type components } from "../api/client";
import { isNotFound, unwrap } from "../api/errors";
import { ArtifactCard, MissingArtifacts, type ArtifactSlot } from "../components/ArtifactCard";
import { FieldGroup, Hash, JsonValue, type Field } from "../components/FieldGroup";
import { Absent, Badge, EmptyState, ErrorState, Loading } from "../components/State";
import {
  CorrectnessBadge,
  DeviceClassBadge,
  DirtyBadge,
  ExitBadge,
  ThrottledBadge,
} from "../components/Status";
import { Timestamp } from "../components/Timestamp";
import { runsFiltersToParams } from "../lib/filters";
import {
  abbreviateSha,
  formatBytes,
  formatDurationSeconds,
  formatElapsed,
  formatTokPerSec,
} from "../lib/format";
import { parseModelName } from "../lib/model";
import { gitNotes, modifiedFiles } from "../lib/provenance";

type Run = components["schemas"]["RunResponse"];

export function RunDetailPage() {
  const { id = "" } = useParams();
  const query = useQuery({
    queryKey: ["run", id],
    queryFn: () => unwrap(api.GET("/api/v1/runs/{id}", { params: { path: { id } } })),
    enabled: id !== "",
  });

  if (query.isPending) return <Loading label="Loading run…" />;
  if (query.isError) {
    if (isNotFound(query.error)) {
      return (
        <EmptyState
          title="Run not found."
          hint={`No run is recorded under the ID ${id}.`}
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

function RunDetail({ run }: { run: Run }) {
  const yesNo = (v: boolean | null | undefined) => (v === null || v === undefined ? null : v ? "yes" : "no");
  const celsius = (v: number | null | undefined) => (v === null || v === undefined ? null : `${v.toFixed(1)} °C`);
  const mhz = (v: number | null | undefined) => (v === null || v === undefined ? null : `${v} MHz`);
  const model = parseModelName(run.model_asset?.original_name ?? "");
  const external = run.device_class === "external";
  // On an external device the lab-only dimensions were never collectable,
  // which is a different statement from "the collector missed them".
  const lab = (value: ReactNode | null | undefined): ReactNode | null =>
    value === null || value === undefined
      ? external
        ? <Absent label="not applicable" />
        : null
      : value;
  const files = modifiedFiles(run.input_parameters);
  const notes = gitNotes(run.input_parameters);

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
        <FieldGroup
          title="Results"
          fields={[
            { label: "Exit status", value: <ExitBadge status={run.exit_status} /> },
            { label: "Correctness", value: <CorrectnessBadge result={run.correctness_result} /> },
            {
              label: "Prefill",
              value: withUnit(formatTokPerSec(run.prefill_tokens_per_sec), "tok/s", "prefill"),
            },
            {
              label: "Decode",
              value: withUnit(formatTokPerSec(run.decode_tokens_per_sec), "tok/s", "decode"),
              hint: "Null when the run measured prefill only; never shown as zero.",
            },
            { label: "Error summary", value: run.error_summary, mono: true },
            {
              label: "Output preview",
              block: true,
              value: run.output_preview ? (
                <pre className="max-h-56 overflow-auto rounded-sm border border-rule bg-wash px-2 py-1.5 font-mono text-[11.5px] whitespace-pre-wrap text-ink-2">
                  {run.output_preview}
                </pre>
              ) : null,
            },
          ]}
        />

        <FieldGroup
          title="Build and workload identity"
          fields={[
            { label: "Git commit", value: <Hash value={run.git_commit_sha} full />, },
            {
              label: "Working tree",
              value: run.git_dirty ? <DirtyBadge /> : <Badge tone="ok" plain>clean</Badge>,
            },
            ...(files ? [modifiedFilesField(files)] : []),
            { label: "Branch", value: run.git_branch, mono: true },
            { label: "Commit time", value: run.git_commit_timestamp ? <Timestamp iso={run.git_commit_timestamp} both /> : null },
            { label: "Commit subject", value: run.git_commit_subject },
            ...(notes ? [{ label: "Commit notes", value: notes } satisfies Field] : []),
            {
              label: "Executable",
              hint: "SHA-256 of the measured binary. Null means the binary was not preserved — it is never a placeholder.",
              value: run.executable_sha256 ? (
                <Hash value={run.executable_sha256} full />
              ) : (
                <span className="text-ink-3 italic" title="No executable hash was recorded for this run.">
                  not preserved
                </span>
              ),
            },
            { label: "Prompt", value: <Hash value={run.prompt_sha256} full /> },
            { label: "Input tokens", value: run.input_token_count.toLocaleString(), mono: true },
            { label: "Output tokens", value: run.output_token_count.toLocaleString(), mono: true },
          ]}
        />

        {run.platform === "android" ? (
          <>
            <FieldGroup
              title="Device state"
              note={external ? "external device — no BSP, driver or thermal capture" : undefined}
              fields={[
                { label: "Platform", value: run.platform, mono: true },
                { label: "Device class", value: <DeviceClassBadge deviceClass={run.device_class} /> },
                { label: "Device serial", value: run.device_serial, mono: true },
                { label: "Device model", value: run.device_model, mono: true },
                { label: "OS", value: run.host_os },
                { label: "Kernel", value: run.host_kernel, mono: true },
                { label: "SoC", value: run.host_cpu_model, mono: true, hint: "Reported as the host CPU model." },
                { label: "CPU count", value: run.host_cpu_count == null ? null : String(run.host_cpu_count), mono: true },
                {
                  label: "Memory",
                  value: run.host_memory_bytes == null ? null : formatBytes(run.host_memory_bytes),
                  mono: true,
                },
                { label: "GPU", value: run.host_accelerator },
                { label: "GPU driver", value: run.host_accelerator_driver, mono: true },
                { label: "BSP version", value: lab(run.bsp_version), mono: true },
                { label: "SUMD driver", value: lab(run.sumd_driver_version), mono: true },
                {
                  label: "Uptime",
                  value: lab(run.device_uptime_seconds == null ? null : formatDurationSeconds(run.device_uptime_seconds)),
                },
                { label: "Battery charging", value: lab(yesNo(run.battery_charging)) },
                { label: "Initial temp.", value: lab(celsius(run.initial_temperature_celsius)) },
                { label: "Max temp.", value: lab(celsius(run.max_temperature_celsius)) },
                {
                  label: "Throttling",
                  value: run.thermal_throttling ? <ThrottledBadge /> : lab(yesNo(run.thermal_throttling)),
                },
              ]}
            />
            <FieldGroup
              title="Performance configuration"
              note={external ? "clocks are not pinnable on an external device" : "pinned clocks"}
              fields={[
                { label: "GPU clock", value: lab(mhz(run.gpu_clock_mhz)), mono: true },
                { label: "MIF clock", value: lab(mhz(run.mif_clock_mhz)), mono: true },
                { label: "INT clock", value: lab(mhz(run.int_clock_mhz)), mono: true },
              ]}
            />
          </>
        ) : (
          <FieldGroup
            title="Host"
            fields={[
              { label: "Platform", value: run.platform, mono: true },
              { label: "Device class", value: <DeviceClassBadge deviceClass={run.device_class} /> },
              { label: "Hostname", value: run.device_serial, mono: true },
              { label: "Machine", value: run.device_model, mono: true },
              { label: "OS", value: run.host_os },
              { label: "Kernel", value: run.host_kernel, mono: true },
              { label: "CPU", value: run.host_cpu_model },
              { label: "CPU count", value: run.host_cpu_count == null ? null : String(run.host_cpu_count), mono: true },
              { label: "Memory", value: run.host_memory_bytes == null ? null : formatBytes(run.host_memory_bytes), mono: true },
              {
                label: "Accelerator",
                value: run.host_accelerator ? (
                  <span title={run.host_accelerator}>{run.host_accelerator}</span>
                ) : null,
              },
              { label: "Accel. driver", value: run.host_accelerator_driver, mono: true },
              {
                label: "Uptime",
                value: run.device_uptime_seconds == null ? null : formatDurationSeconds(run.device_uptime_seconds),
              },
              {
                label: "Throttling",
                value: run.thermal_throttling ? <ThrottledBadge /> : yesNo(run.thermal_throttling),
              },
            ]}
          />
        )}

        <FieldGroup
          title="Model"
          note={run.model_asset?.available ? undefined : "file missing on disk"}
          fields={
            run.model_asset
              ? [
                  { label: "Name", value: run.model_asset.original_name, mono: true },
                  { label: "SHA-256", value: <Hash value={run.model_asset.sha256} full /> },
                  {
                    label: "Availability",
                    value: run.model_asset.available ? (
                      <Badge tone="ok" plain>
                        available
                      </Badge>
                    ) : (
                      <Badge tone="danger">unavailable</Badge>
                    ),
                  },
                  { label: "Asset ID", value: run.model_asset.id, mono: true },
                ]
              : [{ label: "Model", value: null }]
          }
        />

        <FieldGroup
          title="Run metadata"
          fields={[
            { label: "Run ID", value: run.id, mono: true },
            { label: "Started", value: <Timestamp iso={run.started_at} both /> },
            { label: "Finished", value: run.finished_at ? <Timestamp iso={run.finished_at} both /> : null },
            { label: "Elapsed", value: formatElapsed(run.started_at, run.finished_at), mono: true },
            { label: "Repetition", value: String(run.repetition), mono: true },
            {
              label: "Command line",
              block: true,
              value: run.command_line ? (
                <pre className="overflow-x-auto rounded-sm border border-rule bg-wash px-2 py-1.5 font-mono text-[11.5px] whitespace-pre-wrap text-ink-2">
                  {run.command_line}
                </pre>
              ) : null,
            },
            { label: "Command args", block: true, value: <JsonValue value={run.command_args} /> },
            {
              label: "Input parameters",
              block: true,
              value: <JsonValue value={run.input_parameters} />,
            },
            { label: "Environment", block: true, value: <JsonValue value={run.env_vars} /> },
            { label: "Allowlist version", value: run.env_allowlist_version, mono: true },
            { label: "Collector version", value: run.collector_version, mono: true },
          ]}
        />
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
        <p className="mt-1.5 text-ink-3 italic" title="The backend recorded no value for this field.">
          not recorded
        </p>
      )}
    </div>
  );
}

function withUnit(value: string | null, unit: string, tone: "prefill" | "decode"): ReactNode {
  if (!value) return null;
  const colour = tone === "prefill" ? "text-prefill" : "text-decode";
  return (
    <span className="flex items-baseline gap-1">
      <span className={`font-mono font-medium ${colour}`}>{value}</span>
      <span className="text-xs text-ink-3">{unit}</span>
    </span>
  );
}

function modifiedFilesField(files: string[]): Field {
  return {
    label: "Modified files",
    block: true,
    hint: "The files that were uncommitted when this run was measured, from the import manifest.",
    value: <FileList files={files} />,
  };
}

function FileList({ files }: { files: string[] }) {
  return (
    <ul className="max-h-40 space-y-0.5 overflow-auto rounded-sm border border-dirty/25 bg-dirty-soft/50 px-2 py-1.5">
      {files.map((file) => (
        <li key={file} className="font-mono text-[11.5px] break-all text-ink-2">
          {file}
        </li>
      ))}
    </ul>
  );
}
