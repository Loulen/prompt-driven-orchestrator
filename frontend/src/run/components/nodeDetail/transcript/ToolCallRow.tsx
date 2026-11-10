import { useEffect, useRef, useState } from "react";
import { ChevronDown, ChevronRight, LoaderCircle, Wrench } from "lucide-react";
import type { ToolCallDetail } from "../../../../shared/generated/ToolCallDetail";
import type { TranscriptDiffLine } from "../../../../shared/generated/TranscriptDiffLine";
import type { TranscriptDiffStat } from "../../../../shared/generated/TranscriptDiffStat";
import type { TranscriptToolStatus } from "../../../../shared/generated/TranscriptToolStatus";
import { fetchToolCallDetail } from "../../../api/transcript";
import { formatBytes } from "../../../model/transcript";
import { useFocusUnfold } from "../../../store/runSearchStore";

/** A tool call of the transcript, folded on one line (name, summary, lines
 *  changed, output size); its input — or its diff, for a call that replaces
 *  text — and its output are read when it is unfolded, not before. The hit
 *  of the Run's search chosen on it unfolds and outlines it (`focusNonce`). */
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
  focusNonce,
  live = true,
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
  focusNonce?: number;
  /** The iteration still runs: a call without its result is still running.
   *  Ended, it will never get one (the session was reaped mid-call). */
  live?: boolean;
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

  // The search's hit lands here: unfold, read the call.
  useFocusUnfold(focusNonce, () => setOpen(true));
  useEffect(() => {
    if (focusNonce === undefined) return;
    void read();
    // `read` is rebuilt each render; a new choice is the trigger.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focusNonce]);

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
      data-st={status}
      data-entry={index}
      data-focused={focusNonce !== undefined || undefined}
      className="ri-tx__tool"
      style={focusNonce !== undefined ? { background: "color-mix(in oklab, var(--color-st-await) 14%, transparent)" } : undefined}
    >
      <button
        type="button"
        data-testid="transcript-tool-toggle"
        aria-expanded={open}
        onClick={toggle}
        className="ri-tx__line"
      >
        {open ? <ChevronDown size={12} strokeWidth={1.5} aria-hidden /> : <ChevronRight size={12} strokeWidth={1.5} aria-hidden />}
        <Wrench size={12} strokeWidth={1.5} className="ri-tx__wr" aria-hidden />
        <span className="ri-tx__name">{name}</span>
        <span className="ri-tx__sum" title={summary}>
          {summary}
        </span>
        {/* Out of the ellipsis: a long path never hides what the call did. */}
        {diff && (
          <span className="ri-tx__diffn" data-testid="transcript-tool-diffstat">
            <span className="ri-tx__add">+{diff.added}</span> <span className="ri-tx__del">−{diff.removed}</span>
          </span>
        )}
        {outputBytes !== undefined && (
          <span className="ri-tx__size" data-testid="transcript-tool-size">
            {formatBytes(outputBytes)}
          </span>
        )}
        {status === "error" && (
          <span className="ri-tx__st" data-st="failed">
            error
          </span>
        )}
        {status === "pending" && live && (
          <span className="ri-tx__st" data-st="running">
            <LoaderCircle size={11} strokeWidth={1.75} className="d-spin" aria-hidden />
            running
          </span>
        )}
        {status === "pending" && !live && (
          <span className="ri-tx__st" title="The session ended before this call returned">
            no result
          </span>
        )}
      </button>
      {open && (
        <div className="ri-tx__detail" data-testid="transcript-tool-detail">
          {error ? (
            <div className="ri-tx__note text-st-failed">{error}</div>
          ) : detail === null ? (
            <div className="ri-tx__note">Loading…</div>
          ) : (
            <>
              {detail.diff.length > 0 ? <Diff lines={detail.diff} /> : <Block label="Input" text={detail.input} />}
              {detail.output != null ? (
                <Block
                  label={detail.is_error ? "Output (error)" : "Output"}
                  text={detail.output}
                  error={detail.is_error}
                  testId="transcript-tool-output"
                />
              ) : live ? (
                <div className="ri-tx__note" data-testid="transcript-tool-output">
                  <LoaderCircle size={12} strokeWidth={1.5} className="d-spin" aria-hidden />
                  No output yet
                </div>
              ) : (
                <div className="ri-tx__note" data-testid="transcript-tool-output">
                  No output: the session ended before this call returned
                </div>
              )}
              {detail.truncated && <div className="ri-tx__note">Output cut · rest in the session file</div>}
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
    <pre data-testid="transcript-tool-diff" className="ri-tx__code">
      {lines.map((line, i) => (
        <div key={i} data-op={line.op}>
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
    <>
      <div className="d-caps ri-tx__lbl" data-error={error || undefined}>
        {label}
      </div>
      <pre data-testid={testId} data-error={error || undefined} className="ri-tx__code">
        {text}
      </pre>
    </>
  );
}
