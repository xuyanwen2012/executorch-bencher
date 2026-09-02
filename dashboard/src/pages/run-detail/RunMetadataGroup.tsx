import { FieldGroup, JsonValue } from "../../components/FieldGroup";
import { Timestamp } from "../../components/Timestamp";
import { formatElapsed } from "../../lib/format";
import type { Run } from "./shared";

export function RunMetadataGroup({ run }: { run: Run }) {
  return (
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
  );
}
