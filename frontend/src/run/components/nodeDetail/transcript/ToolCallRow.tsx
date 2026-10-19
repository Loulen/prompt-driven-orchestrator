import { useEffect, useRef, useState } from "react";
import { ChevronDown, ChevronRight, Wrench } from "lucide-react";
import type { ToolCallDetail } from "../../../../shared/generated/ToolCallDetail";
import type { TranscriptDiffLine } from "../../../../shared/generated/TranscriptDiffLine";
import type { TranscriptDiffStat } from "../../../../shared/generated/TranscriptDiffStat";
import type { TranscriptToolStatus } from "../../../../shared/generated/TranscriptToolStatus";
import { fetchToolCallDetail } from "../../../api/transcript";
import { formatBytes } from "../../../model/transcript";

/** A tool call of the transcript, folded on one line (name, summary, lines
 *  changed, output size); its input — or its diff, for a call that replaces
 *  text — and its output are read when it is unfolded, not before. */
export default function ToolCallRow({
  runId,
  nodeId,
  iter,
  index,
  name,
  summary,
  status,
  outputBytes,
  diff,
}: {
  runId: string;
  nodeId: string;
  iter: number;
  index: number;
  name: string;
  summary: string;
  status: TranscriptToolStatus;
  outputBytes?: number;
  diff?: TranscriptDiffStat;
}) {
  const [open, setOpen] = useState(false);
  const [detail, setDetail] = useState<ToolCallDetail | null>(null);
  const [error, setError] = useState<string | null>(null);

  const read = () =>
    fetchToolCallDetail(runId, nodeId, iter, index)
      .then((d) => {
        setDetail(d);
        setError(null);
      })
      .catch((e: unknown) => setError(e instanceof Error ? e.message : "cannot read the tool call"));

  const toggle = () => {
    const next = !open;
    setOpen(next);
    // A call still running is read again at each unfold, for its output.
    if (next && (detail === null || detail.output === undefined)) void read();
  };

  // An unfolded call that settles under the reader's eyes shows its output.
  const lastStatus = useRef(status);
  useEffect(() => {
    if (status === lastStatus.current) return;
    lastStatus.current = status;
    if (open && status !== "pending") void read();
    // `read` is rebuilt each render; the status change is the trigger.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [status]);

  return (
    <div
      data-testid="transcript-tool"
      data-status={status}
      className="rounded-md border border-line bg-bg-1"
    >
      <button
        type="button"
        data-testid="transcript-tool-toggle"
        aria-expanded={open}
        onClick={toggle}
        className="flex w-full min-w-0 cursor-pointer items-center gap-1.5 px-2 py-1.5 text-left hover:bg-bg-3"
        style={{ fontSize: "11px" }}
      >
        {open ? (
          <ChevronDown size={11} className="shrink-0 text-fg-4" />
        ) : (
          <ChevronRight size={11} className="shrink-0 text-fg-4" />
        )}
        <Wrench size={10} className="shrink-0 text-fg-4" />
        <span className="shrink-0 font-mono font-medium text-fg-2">{name}</span>
        <span className="min-w-0 truncate font-mono text-fg-4" title={summary}>
          {summary}
        </span>
        {/* Out of the ellipsis: a long path never hides what the call did. */}
        {diff && (
          <span className="shrink-0 font-mono text-fg-4" data-testid="transcript-tool-diffstat">
            +{diff.added} −{diff.removed}
          </span>
        )}
        {outputBytes !== undefined && (
          <span className="shrink-0 whitespace-nowrap font-mono text-fg-4" data-testid="transcript-tool-size">
            · {formatBytes(outputBytes)}
          </span>
        )}
        {status === "error" && (
          <span className="ml-auto shrink-0 text-st-failed" style={{ fontSize: "10px" }}>
            error
          </span>
        )}
        {status === "pending" && (
          <span className="ml-auto shrink-0 text-st-running" style={{ fontSize: "10px" }}>
            running
          </span>
        )}
      </button>
      {open && (
        <div className="border-t border-line px-2 py-1.5" data-testid="transcript-tool-detail">
          {error ? (
            <div className="text-st-failed" style={{ fontSize: "11px" }}>
              {error}
            </div>
          ) : detail === null ? (
            <div className="text-fg-4" style={{ fontSize: "11px" }}>
              Loading…
            </div>
          ) : (
            <>
              {detail.diff.length > 0 ? (
                <Diff lines={detail.diff} />
              ) : (
                <Block label="Input" text={detail.input} />
              )}
              <Block
                label={detail.is_error ? "Output (error)" : "Output"}
                text={detail.output ?? "No output yet: the tool is running."}
                error={detail.is_error}
                testId="transcript-tool-output"
              />
              {detail.truncated && (
                <div className="mt-1 text-fg-4" style={{ fontSize: "10px" }}>
                  Output cut: the rest stays in the session file.
                </div>
              )}
            </>
          )}
        </div>
      )}
    </div>
  );
}

/** The colored line diff of a call that replaces text. */
function Diff({ lines }: { lines: TranscriptDiffLine[] }) {
  return (
    <pre
      data-testid="transcript-tool-diff"
      className="max-h-64 overflow-auto rounded bg-bg-0 py-1.5 font-mono"
      style={{ fontSize: "10.5px", lineHeight: 1.5 }}
    >
      {lines.map((line, i) => (
        <div
          key={i}
          data-op={line.op}
          className={`whitespace-pre-wrap break-words px-1.5 ${
            line.op === "add"
              ? "text-st-done"
              : line.op === "remove"
                ? "text-st-failed"
                : "text-fg-4"
          }`}
        >
          {line.op === "add" ? "+ " : line.op === "remove" ? "- " : "  "}
          {line.text}
        </div>
      ))}
    </pre>
  );
}

function Block({
  label,
  text,
  error = false,
  testId,
}: {
  label: string;
  text: string;
  error?: boolean;
  testId?: string;
}) {
  return (
    <div className="mt-1 first:mt-0">
      <div className="mb-0.5 text-fg-4" style={{ fontSize: "10px" }}>
        {label}
      </div>
      <pre
        data-testid={testId}
        className={`max-h-64 overflow-auto whitespace-pre-wrap break-words rounded bg-bg-0 p-1.5 font-mono ${
          error ? "text-st-failed" : "text-fg-3"
        }`}
        style={{ fontSize: "10.5px", lineHeight: 1.5 }}
      >
        {text}
      </pre>
    </div>
  );
}
