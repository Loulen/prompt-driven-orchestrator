import { AlertCircle, CheckCircle, RotateCcw, Square } from "lucide-react";
import type { NodeState } from "../../../shared/generated/NodeState";
import type { NodeStatus } from "../../../shared/generated/NodeStatus";
import AwaitingBanner from "./AwaitingBanner";
import { isOutputValidationFailure } from "../../model/nodeStatus";

/**
 * The status banners stacked between the toolbar and the terminal: completion
 * released, awaiting user, stale, stopped, interrupted, skipped, frontmatter
 * retry and the two failed variants.
 */
export default function NodeBanners({
  node,
  isArchived,
  selectedIter,
  selectedIterStatus,
  completionReleased,
  nodeAwaitsOnLatestIter,
  terminalLive,
  terminalSpectating,
  canReleaseCompletion,
  isReleasing,
  releaseCompletion,
  markComplete,
  onOpenChildRun,
  killStale,
  restartIteration,
  retry,
}: {
  node: NodeState;
  isArchived?: boolean;
  selectedIter: number;
  selectedIterStatus: NodeStatus;
  completionReleased: boolean;
  nodeAwaitsOnLatestIter: boolean;
  terminalLive: boolean | null;
  terminalSpectating: boolean;
  canReleaseCompletion: boolean;
  isReleasing: boolean;
  releaseCompletion: () => void;
  markComplete: () => void;
  onOpenChildRun?: (childRunId: string) => void;
  killStale: () => void;
  restartIteration: () => void;
  retry: () => void;
}) {
  return (
    <>
      {completionReleased && selectedIterStatus === "running" && (
        <div className="flex items-center gap-2 border-b border-st-running/30 bg-st-running-bg px-3 py-2">
          <CheckCircle size={14} className="shrink-0 text-st-running" />
          <span
            className="text-st-running"
            style={{ fontSize: "11.5px", fontWeight: 500 }}
          >
            Running — completion released. Tell the agent when you are done.
          </span>
        </div>
      )}

      {/* #588 / ADR-0069 — the declared wait, on the iteration on screen. Only
          the latest iteration carries `node.awaiting`; an older awaiting
          iteration (a reaped one) renders the bare shape. A *derived* wait (a
          child awaiting) is a read-time overlay on the node rollup: the latest
          iteration is trusted through `node.awaiting` as well, so the banner
          cannot vanish when the two disagree. */}
      {(selectedIterStatus === "awaiting_user" || nodeAwaitsOnLatestIter) &&
        !completionReleased && (
        <AwaitingBanner
          awaiting={selectedIter === node.iter ? node.awaiting : null}
          terminalLive={terminalLive ?? true}
          terminalSpectating={terminalSpectating}
          canRelease={canReleaseCompletion && !isReleasing}
          onRelease={releaseCompletion}
          onMarkComplete={markComplete}
          onOpenChildRun={onOpenChildRun}
        />
      )}

      {/* Stale banner — HISTORICAL RUNS ONLY since #469 (ADR-0032 §1).
          Nothing in the daemon marks a live node `stale` any more: the idle
          threshold that produced it killed healthy agents mid `docker build`. This
          block is therefore dead code for every new Run, and kept because Runs
          recorded before #469 still project `stale` and must still render as they
          always did. Do NOT "clean it up", and do not wire a producer back in to
          give it something to show.
          Its Retry button is also the only working action here: `Mark complete` is
          rendered for a stale node below but the completion guard refuses the
          transition, so it clicks into the void — a known, deliberately unfixed
          consequence (ADR-0032 § "Ce qu'on ne fait pas": no catch-up). */}
      {node.status === "stale" && (
        <div
          className="flex items-center gap-2 border-b border-st-stale/30 bg-st-stale-bg px-3 py-2"
          data-testid="stale-banner"
        >
          <AlertCircle size={14} className="shrink-0 text-st-stale" />
          <span
            className="flex-1 text-st-stale"
            style={{ fontSize: "11.5px", fontWeight: 500 }}
          >
            Agent idle for &gt;2 min — outputs incomplete
          </span>
          {!isArchived && (
            <div className="flex items-center gap-1">
              <button
                data-testid="stale-stop-btn"
                onClick={killStale}
                className="flex cursor-pointer items-center gap-1 rounded border border-st-stale/40 bg-st-stale/10 px-1.5 py-0.5 text-st-stale transition-colors hover:bg-st-stale/20"
                style={{ fontSize: "10.5px", fontWeight: 500 }}
              >
                <Square size={10} />
                Stop
              </button>
              <button
                data-testid="stale-retry-btn"
                onClick={restartIteration}
                className="flex cursor-pointer items-center gap-1 rounded border border-st-stale/40 bg-st-stale/10 px-1.5 py-0.5 text-st-stale transition-colors hover:bg-st-stale/20"
                style={{ fontSize: "10.5px", fontWeight: 500 }}
              >
                <RotateCcw size={10} />
                Retry
              </button>
            </div>
          )}
        </div>
      )}

      {/* Stopped banner */}
      {node.status === "stopped" && (
        <div className="flex items-center gap-2 border-b border-st-stopped/30 bg-st-stopped-bg px-3 py-2">
          <AlertCircle size={14} className="shrink-0 text-st-stopped" />
          <span
            className="text-st-stopped"
            style={{ fontSize: "11.5px", fontWeight: 500 }}
          >
            Stopped{node.failure_reason ? ` — ${node.failure_reason}` : ""}
          </span>
        </div>
      )}

      {/* Interrupted banner — #598 / ADR-0049. An infra incident killed the
          session (tmux gone, daemon restart, spawn-abort); the work on disk is
          presumed intact and the run is parked, not failed. The one action that
          matters here is Reopen: it re-drives the interrupted node (the daemon
          reopens the run atomically on the node retry). Before this the state
          was a dead end — the placeholder promised "reopen or retry" but no
          button delivered it. Mirrors the `stale` banner's shape; the Reopen
          button shares the toolbar Play's `retry` handler (revives the session,
          flips the terminal back to split). */}
      {node.status === "interrupted" && (
        <div
          className="flex items-center gap-2 border-b border-st-interrupted/30 bg-st-interrupted-bg px-3 py-2"
          data-testid="interrupted-banner"
        >
          <AlertCircle size={14} className="shrink-0 text-st-interrupted" />
          <span
            className="flex-1 text-st-interrupted"
            style={{ fontSize: "11.5px", fontWeight: 500 }}
          >
            Session died{node.failure_reason ? ` — ${node.failure_reason}` : ""} — the
            work is presumed intact
          </span>
          {!isArchived && (
            <button
              data-testid="interrupted-reopen-btn"
              onClick={retry}
              className="flex cursor-pointer items-center gap-1 rounded border border-st-interrupted/40 bg-st-interrupted/10 px-1.5 py-0.5 text-st-interrupted transition-colors hover:bg-st-interrupted/20"
              style={{ fontSize: "10.5px", fontWeight: 500 }}
            >
              <RotateCcw size={10} />
              Reopen
            </button>
          )}
        </div>
      )}

      {/* Skipped banner (#620) — a structurally-unreachable node the resilience
          sweep pruned. Slate, not green: it never ran, its branch was not taken.
          The reason reads at node level (skip_reason), lifted from the skip event. */}
      {node.status === "skipped" && (
        <div className="flex items-center gap-2 border-b border-st-skipped/30 bg-st-skipped-bg px-3 py-2">
          <AlertCircle size={14} className="shrink-0 text-st-skipped" />
          <span
            className="text-st-skipped"
            style={{ fontSize: "11.5px", fontWeight: 500 }}
          >
            Skipped{node.skip_reason ? ` — ${node.skip_reason}` : " — branch not taken"}
          </span>
        </div>
      )}

      {/* Frontmatter retry pending banner (amber) */}
      {node.status === "running" && (node.frontmatter_retries ?? 0) > 0 && (
        <div
          className="flex items-center gap-2 border-b border-st-await/30 bg-st-await-bg px-3 py-2"
          data-testid="frontmatter-retry-banner"
        >
          <AlertCircle size={14} className="shrink-0 text-st-await" />
          <span
            className="text-st-await"
            style={{ fontSize: "11.5px", fontWeight: 500 }}
          >
            Frontmatter mismatch — corrective message sent, awaiting retry
          </span>
        </div>
      )}

      {/* Failed banner — output-validation variant (#490) */}
      {node.status === "failed" && isOutputValidationFailure(node) && (
        <div
          className="flex flex-col gap-1 border-b border-st-failed/30 bg-st-failed-bg px-3 py-2"
          data-testid="output-validation-banner"
        >
          <div className="flex items-center gap-2">
            <AlertCircle size={14} className="shrink-0 text-st-failed" />
            <span
              className="text-st-failed"
              style={{ fontSize: "11.5px", fontWeight: 500 }}
            >
              {/* #490: the reason VERBATIM, not a hard-coded "after retry" — which
                  lied for the `script` fail-fast path, a path that never retries.
                  It also makes the landing order safe: if the status arrives before
                  the evidence, we show the right reason with no list, which is
                  informationally equal to the pre-#490 behaviour. */}
              Failed — {node.failure_reason}
            </span>
          </div>
          {node.frontmatter_violations && node.frontmatter_violations.length > 0 && (
            <ul
              className="mt-0.5 flex flex-col gap-0.5 pl-5 font-mono text-st-failed"
              style={{ fontSize: "10px" }}
              data-testid="frontmatter-violation-list"
            >
              {node.frontmatter_violations.map((v, i) => (
                <li key={i}>
                  {v.port}.{v.field}: {v.reason}
                </li>
              ))}
            </ul>
          )}
          {/* #490: a `script` node failing on a MISSING output used to paint this
              banner with an empty list — the daemon computed the evidence and the
              projector threw it away. */}
          {node.missing_outputs && node.missing_outputs.length > 0 && (
            <ul
              className="mt-0.5 flex flex-col gap-0.5 pl-5 font-mono text-st-failed"
              style={{ fontSize: "10px" }}
              data-testid="missing-output-list"
            >
              {node.missing_outputs.map((m) => (
                <li key={m}>Missing outputs: {m}</li>
              ))}
            </ul>
          )}
        </div>
      )}

      {/* Failed banner — generic. Same predicate, inverted: two different tests here
          would let both banners fire (or neither). */}
      {node.status === "failed" && !isOutputValidationFailure(node) && (
        <div className="flex items-center gap-2 border-b border-st-failed/30 bg-st-failed-bg px-3 py-2">
          <AlertCircle size={14} className="shrink-0 text-st-failed" />
          <span
            className="text-st-failed"
            style={{ fontSize: "11.5px", fontWeight: 500 }}
          >
            Failed{node.failure_reason ? ` — ${node.failure_reason}` : ""}
          </span>
        </div>
      )}
    </>
  );
}
