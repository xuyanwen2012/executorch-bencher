import { type Field, FieldGroup, Hash } from "../../components/FieldGroup";
import { Badge } from "../../components/State";
import { DirtyBadge } from "../../components/Status";
import { Timestamp } from "../../components/Timestamp";
import { gitNotes, modifiedFiles } from "../../lib/provenance";
import type { Run } from "./shared";

export function BuildIdentityGroup({ run }: { run: Run }) {
  const files = modifiedFiles(run.input_parameters);
  const notes = gitNotes(run.input_parameters);
  return (
    <FieldGroup
      title="Build and workload identity"
      fields={[
        { label: "Git commit", value: <Hash value={run.git_commit_sha} full /> },
        {
          label: "Working tree",
          value: run.git_dirty ? (
            <DirtyBadge />
          ) : (
            <Badge tone="ok" plain>
              clean
            </Badge>
          ),
        },
        ...(files ? [modifiedFilesField(files)] : []),
        { label: "Branch", value: run.git_branch, mono: true },
        {
          label: "Commit time",
          value: run.git_commit_timestamp ? <Timestamp iso={run.git_commit_timestamp} both /> : null,
        },
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
