import { useState } from "react";
import type { components } from "../api/client";
import { formatBytes } from "../lib/format";
import { Badge } from "./State";

type ArtifactView = components["schemas"]["ArtifactView"];

export interface ArtifactSlot {
  slot: string;
  artifact: ArtifactView | null | undefined;
}

/** Inline viewing is offered for text artifacts at or below this size. */
export const INLINE_PREVIEW_MAX_BYTES = 256 * 1024;

export function canViewInline(artifact: Pick<ArtifactView, "available" | "media_type" | "size_bytes">): boolean {
  return (
    artifact.available &&
    typeof artifact.media_type === "string" &&
    artifact.media_type.toLowerCase().startsWith("text/") &&
    artifact.size_bytes <= INLINE_PREVIEW_MAX_BYTES
  );
}

type Content =
  | { state: "idle" }
  | { state: "loading" }
  | { state: "shown"; text: string }
  | { state: "error"; message: string };

export function ArtifactCard({ slot, artifact }: { slot: string; artifact: ArtifactView | null | undefined }) {
  const [content, setContent] = useState<Content>({ state: "idle" });

  const view = async () => {
    if (!artifact) return;
    setContent({ state: "loading" });
    try {
      const response = await fetch(artifact.content_url);
      if (!response.ok) {
        setContent({ state: "error", message: `the backend answered HTTP ${response.status}` });
        return;
      }
      setContent({ state: "shown", text: await response.text() });
    } catch (err) {
      setContent({ state: "error", message: err instanceof Error ? err.message : String(err) });
    }
  };

  if (!artifact) return null;
  const inline = canViewInline(artifact);

  return (
    <article className="panel flex flex-col overflow-hidden" data-testid={`artifact-${slot}`}>
      <div className="flex items-center justify-between gap-2 border-b border-rule bg-wash px-3 py-1.5">
        <h3 className="eyebrow text-ink-2">{slot}</h3>
        {artifact.available ? (
          <Badge tone="ok" plain>
            available
          </Badge>
        ) : (
          <Badge tone="danger">unavailable</Badge>
        )}
      </div>

      <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-4 gap-y-1 px-3 py-2 text-xs">
        <Row label="kind">
          <span className="font-mono">{artifact.kind}</span>
        </Row>
        <Row label="filename">
          <span className="font-mono break-all" title={artifact.original_filename ?? undefined}>
            {artifact.original_filename ?? <span className="text-ink-3 italic">none recorded</span>}
          </span>
        </Row>
        <Row label="size">
          <span className="font-mono">{formatBytes(artifact.size_bytes)}</span>
        </Row>
        <Row label="media type">
          <span className="font-mono">
            {artifact.media_type ?? <span className="text-ink-3 italic">unknown</span>}
          </span>
        </Row>
        <Row label="compression">
          <span className="font-mono">{artifact.compression}</span>
        </Row>
      </dl>

      {artifact.available ? (
        <div className="mt-auto flex items-center gap-3 border-t border-rule px-3 py-1.5">
          <a
            href={artifact.download_url}
            download
            className="eyebrow text-ink-2 underline decoration-rule-strong underline-offset-2 hover:text-prefill"
          >
            Download
          </a>
          {inline ? (
            content.state === "shown" ? (
              <button
                type="button"
                onClick={() => setContent({ state: "idle" })}
                className="eyebrow text-ink-2 underline decoration-rule-strong underline-offset-2 hover:text-prefill"
              >
                Hide content
              </button>
            ) : (
              <button
                type="button"
                onClick={() => void view()}
                className="eyebrow text-ink-2 underline decoration-rule-strong underline-offset-2 hover:text-prefill"
              >
                {content.state === "loading" ? "Loading…" : "View content"}
              </button>
            )
          ) : (
            <span
              className="eyebrow text-ink-3"
              title="Inline viewing is offered for text files up to 256 KiB."
            >
              Download only
            </span>
          )}
        </div>
      ) : (
        <p className="mt-auto border-t border-danger/25 bg-danger-soft px-3 py-1.5 text-xs text-danger">
          The stored file is missing, so it cannot be viewed or downloaded.
        </p>
      )}

      {content.state === "shown" ? (
        <pre className="max-h-80 overflow-auto border-t border-rule bg-paper px-3 py-2 font-mono text-[11.5px] whitespace-pre-wrap text-ink-2">
          {content.text}
        </pre>
      ) : null}
      {content.state === "error" ? (
        <p className="border-t border-danger/25 bg-danger-soft px-3 py-1.5 text-xs text-danger">
          Could not load the content: {content.message}.
        </p>
      ) : null}
    </article>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <>
      <dt className="eyebrow pt-px text-ink-3">{label}</dt>
      <dd className="min-w-0 text-ink-2">{children}</dd>
    </>
  );
}

/** The slots this run carries nothing for: stated once, quietly. */
export function MissingArtifacts({ slots }: { slots: readonly string[] }) {
  if (slots.length === 0) return null;
  return (
    <p className="mt-3 flex flex-wrap items-baseline gap-x-2 gap-y-1 text-xs text-ink-3">
      <span className="eyebrow">Not attached</span>
      {slots.map((slot) => (
        <span key={slot} className="rounded-[2px] border border-dashed border-rule px-1.5 py-px font-mono">
          {slot}
        </span>
      ))}
    </p>
  );
}
