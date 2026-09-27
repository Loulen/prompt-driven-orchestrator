import { AlertCircle, CheckCircle, LoaderCircle } from "lucide-react";
import type { MarkVerdict, ReleaseVerdict } from "../../store/useNodeRun";
import { verdictHeadline, verdictTone } from "../../model/nodeStatus";

/**
 * The verdict of a *Mark complete* click, rendered at the gesture (#490).
 *
 * `data-*` attributes rather than copy are the assertion surface: a level-5 driver
 * asserts `data-recoverable="true"` for "still your turn" and `"false"` for "the node
 * is failed now", so the journey survives a rewording.
 */
export function MarkCompleteVerdict({ verdict }: { verdict: MarkVerdict }) {
  const tone = verdictTone(verdict);
  const toneClass = {
    done: "border-st-done/30 bg-st-done-bg text-st-done",
    await: "border-st-await/30 bg-st-await-bg text-st-await",
    failed: "border-st-failed/30 bg-st-failed-bg text-st-failed",
    stopped: "border-line-strong bg-bg-3 text-fg-3",
  }[tone];
  const refused = verdict.kind === "refused" ? verdict : null;

  return (
    <div
      className={`flex flex-col gap-1 rounded-md border px-2.5 py-1.5 ${toneClass}`}
      style={{ fontSize: "10.5px" }}
      data-testid="mark-complete-verdict"
      data-verdict={verdict.kind}
      data-slug={refused?.slug ?? ""}
      data-recoverable={refused ? String(refused.recoverable) : ""}
    >
      <div className="flex items-start gap-1.5">
        {tone === "done" ? (
          <CheckCircle size={12} className="mt-px shrink-0" />
        ) : (
          <AlertCircle size={12} className="mt-px shrink-0" />
        )}
        <span>{verdictHeadline(verdict)}</span>
      </div>
      {refused && refused.missing.length > 0 && (
        <ul
          className="flex flex-col gap-0.5 pl-5 font-mono"
          data-testid="verdict-missing-list"
        >
          {/* The `Missing outputs:` prefix is load-bearing — the gating e2e spec
              matches `/^Missing outputs:/`. */}
          <li>Missing outputs: {refused.missing.join(", ")}</li>
        </ul>
      )}
      {refused && refused.violations.length > 0 && (
        <ul
          className="flex flex-col gap-0.5 pl-5 font-mono"
          data-testid="verdict-violation-list"
        >
          {refused.violations.map((v, i) => (
            <li key={i}>
              {v.port}.{v.field}: {v.reason}
            </li>
          ))}
        </ul>
      )}
      {refused?.recoverable === true && (
        <span className="pl-5">Fix the above, then click Mark complete again.</span>
      )}
      {refused?.recoverable === false && (
        <span className="pl-5">Resume the run to try again.</span>
      )}
    </div>
  );
}

export function ReleaseCompletionVerdict({
  verdict,
}: {
  verdict: ReleaseVerdict | { kind: "released" };
}) {
  const refused = verdict.kind === "refused" ? verdict : null;
  const failed = verdict.kind === "refused" || verdict.kind === "error";
  const toneClass =
    verdict.kind === "released"
      ? "border-st-running/30 bg-st-running-bg text-st-running"
      : failed
        ? "border-st-failed/30 bg-st-failed-bg text-st-failed"
        : "border-line-strong bg-bg-3 text-fg-3";
  const headline =
    verdict.kind === "released"
      ? "Completion released — the agent will complete when ready."
      : verdict.kind === "pending"
        ? "Releasing completion…"
        : verdict.kind === "error"
          ? `Could not reach the daemon — ${verdict.message}`
          : `Release refused — ${verdict.message}`;

  return (
    <div
      className={`flex flex-col gap-1 rounded-md border px-2.5 py-1.5 ${toneClass}`}
      style={{ fontSize: "10.5px" }}
      data-testid="release-verdict"
      data-verdict={verdict.kind}
      data-slug={refused?.slug ?? ""}
      data-recoverable={refused ? String(refused.recoverable) : ""}
    >
      <div className="flex items-start gap-1.5">
        {verdict.kind === "released" ? (
          <CheckCircle size={12} className="mt-px shrink-0" />
        ) : verdict.kind === "pending" ? (
          <LoaderCircle size={12} className="mt-px shrink-0 animate-spin" />
        ) : (
          <AlertCircle size={12} className="mt-px shrink-0" />
        )}
        <span>{headline}</span>
      </div>
      {verdict.kind === "released" && (
        <span className="pl-5">
          Now tell the agent in the terminal that you are done. Mark complete still
          forces it.
        </span>
      )}
      {verdict.kind === "refused" && (
        <span className="pl-5">Retry the node, then release again.</span>
      )}
    </div>
  );
}
