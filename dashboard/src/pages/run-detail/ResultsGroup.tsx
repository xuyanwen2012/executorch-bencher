import { FieldGroup } from "../../components/FieldGroup";
import { CorrectnessBadge, ExitBadge } from "../../components/Status";
import { formatTokPerSec } from "../../lib/format";
import { type Run, withUnit } from "./shared";

export function ResultsGroup({ run }: { run: Run }) {
  return (
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
  );
}
