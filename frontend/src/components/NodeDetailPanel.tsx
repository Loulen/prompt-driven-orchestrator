import { useCallback, useMemo, useState } from "react";
import {
  CheckCircle,
  AlertCircle,
  ChevronDown,
  ChevronRight,
  Square,
  RotateCcw,
  Play,
  Maximize2,
} from "lucide-react";
import type { IterationInfo, NodeState, NodeStatus } from "../types";
import { artifactUrl } from "../api";
import type { PortIO, FileInfo } from "../api";
import type { PortType } from "../types";
import { useNodeRun } from "../hooks/useNodeRun";
import type { MarkVerdict } from "../hooks/useNodeRun";
import {
  ResizablePanelGroup,
  ResizablePanel,
  ResizableHandle,
} from "./ui/resizable";
import {
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuItem,
} from "./ui/dropdown-menu";
import MarkdownArtifactModal from "./MarkdownArtifactModal";
import type { ArtifactSource } from "./MarkdownArtifactModal";
import ImageLightbox from "./ImageLightbox";
import TmuxTerminal from "./TmuxTerminal";

const STATUS_LABELS: Record<NodeStatus, string> = {
  pending: "Pending",
  running: "Running",
  awaiting_user: "Awaiting User",
  completed: "Completed",
  failed: "Failed",
  stopped: "Stopped",
  stale: "Stale",
  interrupted: "Interrupted",
};

interface Props {
  node: NodeState;
  runId: string;
  isArchived?: boolean;
  nodeName?: string | null;
  initialTerminalExpanded?: boolean;
}

// The terminal inset has three mutually exclusive display modes (#346):
//   - "split"     : terminal ~45% / detail pane ~55% (default for a live node)
//   - "expanded"  : terminal full-frame, detail pane hidden (user gesture, #270)
//   - "minimized" : terminal collapsed to a thin bar, Outputs take the full
//                   height (default when opening a node whose session ended)
// An enum (not two orthogonal booleans) makes the illegal
// `{minimized + expanded}` state unrepresentable.
type TerminalView = "minimized" | "split" | "expanded";

/**
 * Three tones, because the three answers demand three different reactions:
 * `await` = it is still your turn, `failed` = the node is failed *now*,
 * `stopped` = refused with no state change, or indeterminable.
 */
function verdictTone(v: MarkVerdict): "await" | "failed" | "stopped" | "done" {
  switch (v.kind) {
    case "completed":
      return "done";
    case "noop":
      return "stopped";
    case "pending":
      return "stopped";
    case "error":
      return "failed";
    case "refused":
      if (v.recoverable === true) return "await";
      if (v.recoverable === false) return "failed";
      return "stopped";
  }
}

function verdictHeadline(v: MarkVerdict): string {
  switch (v.kind) {
    case "pending":
      return "Marking complete…";
    case "completed":
      return "Marked complete.";
    case "noop":
      return `Already complete — nothing to do (${v.reason})`;
    case "error":
      return `Could not reach the daemon — ${v.message}`;
    case "refused":
      if (v.recoverable === true) return `Refused, still your turn — ${v.message}`;
      if (v.recoverable === false) return `Refused, the node is now failed — ${v.message}`;
      return `Refused — ${v.message}`;
  }
}

/**
 * Does this node's failure come from output validation? (#490)
 *
 * `includes`, **not** an equality or a `startsWith`: the two reasons are
 * `"output validation failed"` (after retry) and `"script output validation failed"`
 * (the `script` fail-fast), and the second neither equals nor starts with the first.
 * Both banner gates use this one predicate, inverted — two separate tests would let
 * both fire, or neither.
 */
function isOutputValidationFailure(node: NodeState): boolean {
  return node.failure_reason?.includes("output validation failed") ?? false;
}

/**
 * The verdict of a *Mark complete* click, rendered at the gesture (#490).
 *
 * `data-*` attributes rather than copy are the assertion surface: a level-5 driver
 * asserts `data-recoverable="true"` for "still your turn" and `"false"` for "the node
 * is failed now", so the journey survives a rewording.
 */
function MarkCompleteVerdict({ verdict }: { verdict: MarkVerdict }) {
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

// The session is settled — the tmux session is gone, so the terminal WebSocket
// would attach to a dead session. `{completed, failed, stopped, interrupted}` is
// exactly the "settled" tier of `pollInterval` (5s). `interrupted` belongs here
// (#598 / ADR-0049): its tmux session is *definitively* dead (that death is what
// produced the status), so attaching would only spam "can't find session" — open
// minimized and prioritise the Outputs, exactly like the other settled states.
// A Retry revives the session and flips back to split (`onRetryStarted`). `stale`
// is EXCLUDED unless archived: its tmux session is typically still alive and
// recovery happens *inside* the terminal (nudge / Stop / Retry). An archived run
// overrides everything (its worktree + session are torn down). An unknown future
// status falls on the non-terminated (live) side.
function nodeSessionEnded(status: NodeStatus, isArchived?: boolean): boolean {
  if (isArchived) return true;
  return (
    status === "completed" ||
    status === "failed" ||
    status === "stopped" ||
    status === "interrupted"
  );
}

interface ModalState {
  portName: string;
  files: FileInfo[];
  portKind: "input" | "output";
  portType: PortType;
}

export default function NodeDetailPanel({
  node,
  runId,
  isArchived,
  nodeName,
  initialTerminalExpanded,
}: Props) {
  const [modal, setModal] = useState<ModalState | null>(null);
  // Seed at mount only (no reactive effect on status): the issue trigger is
  // "clicking the node" (= selection / mount), not a live transition. A node
  // is `key`-ed by node_id at both mount sites, so selecting another terminated
  // node remounts → re-seeds → minimized; a node that settles under the user's
  // eyes is NOT folded reactively (that would be jarring — cf. #270 "agrandir =
  // explicit gesture"). Retry clears `minimized` explicitly (session revives).
  const [terminalView, setTerminalView] = useState<TerminalView>(() => {
    if (initialTerminalExpanded) return "expanded"; // seam #270/#129 (dead in prod)
    return nodeSessionEnded(node.status, isArchived) ? "minimized" : "split";
  });
  const [userSelectedIter, setUserSelectedIter] = useState<{
    nodeId: string;
    iter: number;
  } | null>(null);

  const selectedIter =
    userSelectedIter?.nodeId === node.node_id
      ? userSelectedIter.iter
      : node.iter;

  const setSelectedIter = useCallback(
    (iter: number) => {
      setUserSelectedIter({ nodeId: node.node_id, iter });
    },
    [node.node_id],
  );

  // The retry handlers used to call `setTerminalView("split")` inline. The mode
  // stays presentational state here; it travels into `useNodeRun` as a *stable*
  // callback (`useCallback` with no deps, exactly as stable as the `setTerminalView`
  // setter it replaces), so the retry callbacks keep the identity they had.
  const showTerminalSplit = useCallback(() => setTerminalView("split"), []);

  // Every read and command of this panel is iter-scoped, so the resolved
  // `selectedIter` is an argument: the panel owns the IterSelector's state and the
  // hook owns the orchestration. One owner, no second copy to keep in sync.
  const {
    promptText,
    inputs,
    outputs,
    markVerdict,
    actionVerdict,
    retryConfirm,
    stop,
    retry,
    confirmRetry,
    cancelRetry,
    start,
    markComplete,
    killStale,
    restartStale,
  } = useNodeRun(runId, node, selectedIter, {
    isArchived,
    onRetryStarted: showTerminalSplit,
  });

  const sessionName = `pdo-${runId}-${node.node_id}-iter-${selectedIter}`;
  const hasMultipleIters = (node.iterations?.length ?? 0) > 1;
  const showTerminal = node.status !== "pending";

  // #369: the I/O poll (`setInputs`/`setOutputs`) re-renders this panel every
  // tick (1s live, 5s settled). Building the modal's `source` prop as an inline
  // object literal handed the child a fresh reference on every one of those
  // re-renders; the modal keyed a fetch effect on that reference, so it re-ran
  // `fetchNodeIO` + `setFiles(new array)` + `setFileIndex(0)` on every tick,
  // which momentarily emptied the body and unmounted/remounted the rendered
  // markdown (mermaid diagram → back to its empty `aria-busy` state → flicker).
  // Memoize on the structural keys so the reference stays stable across ticks
  // and only changes when the underlying identity actually does.
  const modalSource = useMemo<ArtifactSource | null>(() => {
    if (!modal) return null;
    return node.iterations && node.iterations.length > 1
      ? {
          kind: "iter-nav",
          nodeId: node.node_id,
          portKind: modal.portKind,
          iterations: node.iterations,
          initialIter: selectedIter,
        }
      : { kind: "static", files: modal.files };
  }, [modal, node.iterations, node.node_id, selectedIter]);

  // #369: a stable `onClose` (setModal is a stable useState setter, so no deps).
  // The panel re-renders on every I/O poll tick; an inline `() => setModal(null)`
  // handed the modal a fresh prop each tick, re-rendering it needlessly. A constant
  // identity lets the modal's memoised markdown props do their job (no diagram
  // remount, no flicker).
  const closeModal = useCallback(() => setModal(null), []);

  return (
    <aside className="flex h-full flex-col bg-bg-2">
      {/* Header */}
      <div className="border-b border-line px-3 py-2">
        <div className="flex items-center gap-2">
          <span className="font-medium text-fg" style={{ fontSize: "12.5px" }}>
            {nodeName ?? node.node_id}
          </span>
          <span
            className="rounded border border-line-strong bg-bg-3 px-1.5 py-0.5 text-fg-3"
            style={{ fontSize: "10px", fontWeight: 500 }}
          >
            {STATUS_LABELS[node.status] ?? node.status}
          </span>
        </div>
        <div className="mt-0.5 font-mono text-fg-4" style={{ fontSize: "9px" }}>
          {node.node_id}
        </div>
        <div
          className="mt-0.5 flex items-center gap-1 font-mono text-fg-4"
          style={{ fontSize: "10px" }}
        >
          {hasMultipleIters ? (
            <IterSelector
              iterations={node.iterations}
              selectedIter={selectedIter}
              onSelect={setSelectedIter}
            />
          ) : (
            <span>iter {node.iter}</span>
          )}
          {node.started_at && (
            <span> · started {formatTime(node.started_at)}</span>
          )}
          {node.completed_at && (
            <span> · ended {formatTime(node.completed_at)}</span>
          )}
        </div>
      </div>

      {!isArchived && (
        <div
          className="flex items-center gap-1.5 border-b border-line px-3 py-1.5"
          data-testid="node-controls"
        >
          <button
            data-testid="stop-btn"
            disabled={node.status !== "running"}
            onClick={node.status === "running" ? stop : undefined}
            className={
              node.status === "running"
                ? "flex cursor-pointer items-center gap-1 rounded border border-st-failed/40 bg-st-failed/10 px-2 py-0.5 text-st-failed transition-colors hover:bg-st-failed/20"
                : "flex items-center gap-1 rounded border border-line bg-bg-3 px-2 py-0.5 text-fg-4 opacity-50"
            }
            style={{ fontSize: "10.5px", fontWeight: 500 }}
          >
            <Square size={10} />
            Stop
          </button>
          {node.status === "pending" && (
            <button
              data-testid="start-btn"
              onClick={start}
              className={RETRY_BUTTON_CLASS}
              style={RETRY_BUTTON_STYLE}
            >
              <Play size={10} />
              Start
            </button>
          )}
          <RetryPlayButton status={node.status} onClick={retry} />
        </div>
      )}

      {/* #487: the refusal of a Retry/Play or Start click, rendered AT the gesture.
          Before this the daemon's 409 ("resume the run first" / "session cap
          reached") was swallowed, so the click looked like it did nothing. */}
      {!isArchived && actionVerdict && (
        <div
          className="flex items-start gap-2 border-b border-st-failed/30 bg-st-failed/10 px-3 py-2"
          data-testid="action-verdict"
          data-action={actionVerdict.action}
        >
          <AlertCircle size={14} className="mt-0.5 shrink-0 text-st-failed" />
          <span className="text-st-failed" style={{ fontSize: "11.5px", fontWeight: 500 }}>
            {actionVerdict.action === "retry" ? "Retry refused" : "Start refused"} —{" "}
            {actionVerdict.message}
          </span>
        </div>
      )}

      {/* Awaiting user banner */}
      {node.status === "awaiting_user" && (
        <div className="flex items-center gap-2 border-b border-st-await/30 bg-st-await-bg px-3 py-2">
          <AlertCircle size={14} className="shrink-0 text-st-await" />
          <span
            className="text-st-await"
            style={{ fontSize: "11.5px", fontWeight: 500 }}
          >
            Awaiting user — interact in the terminal below, then mark complete
          </span>
        </div>
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
                onClick={restartStale}
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

      {(() => {
        const terminalPane = (
          <div
            className="flex h-full flex-col overflow-hidden"
            data-testid="terminal-pane-wrapper"
          >
            {showTerminal ? (
              <TmuxTerminal
                session={sessionName}
                expanded={terminalView === "expanded"}
                onExpand={() =>
                  setTerminalView((v) => (v === "expanded" ? "split" : "expanded"))
                }
                status={node.status}
              />
            ) : (
              <div className="flex h-full flex-col" data-testid="pending-placeholder">
                <div
                  className="flex items-center gap-1.5 border-b border-line px-3 py-1.5 text-fg-3"
                  style={{ fontSize: "11px" }}
                >
                  <span className="h-1.5 w-1.5 rounded-full bg-fg-5" />
                  Terminal
                </div>
                <div className="flex flex-1 items-center justify-center bg-bg-0">
                  <span className="text-fg-4" style={{ fontSize: "11px" }}>
                    {terminalPlaceholder(node)}
                  </span>
                </div>
              </div>
            )}
          </div>
        );

        const detailsPane = (
          <div
            className="flex h-full flex-col overflow-auto"
            data-testid="details-pane"
          >
            {/* Actions */}
            <div className="flex flex-col gap-1.5 px-3 py-2">
              {/* `interrupted` is included (#598 / ADR-0049): an interrupted
                  interactive node parks the run `awaiting_user`, so the "take the
                  artifacts as they are" escape must stay reachable — the daemon
                  embeds the run reopen on `mark_node_done` too, and the output
                  guard rejects gracefully (rendered at the gesture) if the work
                  is genuinely incomplete. */}
              {(node.status === "awaiting_user" || node.status === "running" || node.status === "failed" || node.status === "stale" || node.status === "interrupted") && !isArchived && (
                <>
                  <button
                    onClick={markComplete}
                    data-testid="mark-complete-btn"
                    className="flex w-full cursor-pointer items-center justify-center gap-1.5 rounded-md border border-st-done/40 bg-st-done-bg px-3 py-1.5 text-st-done transition-colors hover:border-st-done/60 hover:bg-st-done/20"
                    style={{ fontSize: "11.5px", fontWeight: 500 }}
                  >
                    <CheckCircle size={12} />
                    Mark complete
                  </button>

                  {/* #490: the verdict of the click, AT the gesture. Rendered for
                      every outcome including `pending`, so nothing ever blinks out
                      without a replacement. */}
                  {markVerdict && markVerdict.iter === selectedIter && (
                    <MarkCompleteVerdict verdict={markVerdict} />
                  )}
                </>
              )}
            </div>

            {/* Inputs section */}
            {inputs.length > 0 && (
              <IOSection
                title="Inputs"
                ports={inputs}
                runId={runId}
                onOpenFile={(portName, files, portType) =>
                  setModal({ portName, files, portKind: "input", portType })
                }
              />
            )}

            {/* Outputs section */}
            {outputs.length > 0 && (
              <IOSection
                title="Outputs"
                ports={outputs}
                runId={runId}
                showFrontmatter
                onOpenFile={(portName, files, portType) =>
                  setModal({ portName, files, portKind: "output", portType })
                }
              />
            )}

            {/* Initial Prompt */}
            <PromptSection
              promptText={promptText}
              status={node.status}
              isArchived={isArchived}
            />
          </div>
        );

        // Minimized view (#346): the session has ended, so prioritise the
        // Outputs. Render a separate declarative subtree — a thin clickable bar
        // + the full-height details pane — with NO `TmuxTerminal` and NO
        // `ResizablePanelGroup`. The non-remount invariant of `TmuxTerminal`
        // (below) is a *live-session* concern (a WS reconnect re-pushes Claude's
        // prompt); for a settled session the WS points at a dead session, so
        // NOT mounting it is harmless and avoids a pointless attach. Isolating
        // `minimized` leaves the live path (split / expanded) fully intact.
        if (terminalView === "minimized") {
          return (
            <div
              className="flex min-h-0 flex-1 flex-col"
              data-testid="terminal-minimized"
            >
              <button
                type="button"
                onClick={() => setTerminalView("split")}
                data-testid="term-restore"
                className="flex items-center gap-1.5 border-b border-line px-3 py-1.5 text-fg-3 transition-colors hover:text-fg-2"
                style={{ fontSize: "11px" }}
                title="Agrandir le terminal"
              >
                <span className="h-1.5 w-1.5 rounded-full bg-fg-5" />
                Terminal
                <span className="flex-1" />
                <Maximize2 size={12} />
              </button>
              {/* detailsPane has an inner h-full wrapper → frame it flex-1 /
                  min-h-0 under the bar so it takes the remaining height. */}
              <div className="min-h-0 flex-1 overflow-hidden">{detailsPane}</div>
            </div>
          );
        }

        // Keep `TmuxTerminal` mounted across the fullscreen toggle: render
        // the same `<ResizablePanelGroup>` parent in both modes and only
        // conditionally render the details panel + handle. React's reconciler
        // matches the terminal panel at position 0 in both renders, so the
        // WebSocket and xterm instance survive the toggle. Conditional panels
        // with stable `id` + `order` props are the documented pattern for
        // react-resizable-panels.
        return (
          <ResizablePanelGroup
            orientation="vertical"
            className="min-h-0 flex-1"
            data-testid={terminalView === "expanded" ? "terminal-fullsize" : undefined}
          >
            <ResizablePanel
              id="terminal"
              defaultSize={terminalView === "expanded" ? 100 : 45}
              minSize="100px"
            >
              {terminalPane}
            </ResizablePanel>
            {terminalView !== "expanded" && (
              <>
                <ResizableHandle />
                <ResizablePanel
                  id="details"
                  defaultSize={55}
                  minSize="100px"
                >
                  {detailsPane}
                </ResizablePanel>
              </>
            )}
          </ResizablePanelGroup>
        );
      })()}

      {modal && modalSource && (
        <MarkdownArtifactModal
          runId={runId}
          portName={modal.portName}
          portType={modal.portType}
          source={modalSource}
          onClose={closeModal}
        />
      )}

      {retryConfirm && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
          data-testid="retry-confirm-backdrop"
          onClick={cancelRetry}
        >
          <div
            className="w-[360px] rounded-lg border border-line bg-bg-2 p-4 shadow-lg"
            style={{ fontSize: "12px" }}
            onClick={(e) => e.stopPropagation()}
          >
            <h3 className="font-medium text-fg" style={{ fontSize: "13px" }}>
              Retry this node?
            </h3>
            <p className="mt-2 text-fg-3" style={{ fontSize: "11.5px" }}>
              This will reset {retryConfirm.affectedCount} downstream{" "}
              {retryConfirm.affectedCount === 1 ? "node" : "nodes"} with
              artifacts. Continue?
            </p>
            <div className="mt-4 flex justify-end gap-2">
              <button
                data-testid="retry-confirm-cancel"
                onClick={cancelRetry}
                className="rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 transition-colors hover:bg-bg-4"
                style={{ fontSize: "11.5px" }}
              >
                Cancel
              </button>
              <button
                data-testid="retry-confirm-ok"
                onClick={confirmRetry}
                className="rounded-md bg-accent px-3 py-1.5 text-white transition-colors hover:bg-accent/80"
                style={{ fontSize: "11.5px" }}
              >
                Retry
              </button>
            </div>
          </div>
        </div>
      )}
    </aside>
  );
}

const RETRY_BUTTON_CLASS =
  "flex cursor-pointer items-center gap-1 rounded border border-line-strong bg-bg-3 px-2 py-0.5 text-fg-2 transition-colors hover:bg-bg-4";
const RETRY_BUTTON_STYLE = { fontSize: "10.5px", fontWeight: 500 } as const;

function RetryPlayButton({
  status,
  onClick,
}: {
  status: NodeStatus;
  onClick: () => void;
}) {
  if (status === "running") {
    return (
      <button
        data-testid="retry-btn"
        onClick={onClick}
        className={RETRY_BUTTON_CLASS}
        style={RETRY_BUTTON_STYLE}
      >
        <RotateCcw size={10} />
        Retry
      </button>
    );
  }

  if (status === "completed") {
    return (
      <button
        data-testid="play-retry-btn"
        onClick={onClick}
        className={RETRY_BUTTON_CLASS}
        style={RETRY_BUTTON_STYLE}
      >
        <RotateCcw size={10} />
        Retry
      </button>
    );
  }

  // #598 / ADR-0049: `interrupted` joins the "Play" set. The node-level retry
  // re-drives the interrupted work — the daemon embeds the run reopen atomically
  // (`node_retry` → `embed_reopen_for_targeted_command`), so the click that used
  // to hit "resume the run first" now just works. Without this the panel offered
  // no way out of an interrupted node (issue: an interactive node with a dead
  // session had no resume affordance).
  if (
    status === "failed" ||
    status === "stopped" ||
    status === "stale" ||
    status === "interrupted"
  ) {
    return (
      <button
        data-testid="play-retry-btn"
        onClick={onClick}
        className={RETRY_BUTTON_CLASS}
        style={RETRY_BUTTON_STYLE}
      >
        <Play size={10} />
        Play
      </button>
    );
  }

  return null;
}

function PromptSection({
  promptText,
  status,
  isArchived,
}: {
  promptText: string | null;
  status: NodeStatus;
  isArchived?: boolean;
}) {
  const [expanded, setExpanded] = useState(false);

  return (
    <div className="border-t border-line">
      <button
        type="button"
        onClick={() => setExpanded(!expanded)}
        className="flex w-full cursor-pointer items-center gap-1.5 px-3 py-1.5 text-fg-3 transition-colors hover:text-fg-2"
        style={{ fontSize: "11px" }}
        data-testid="prompt-toggle"
      >
        {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        Initial Prompt
      </button>
      {expanded && (
        <pre
          className="prompt-block overflow-auto bg-bg-0 p-2 font-mono text-fg-3"
          style={{ fontSize: "10px", lineHeight: "1.5" }}
        >
          {promptText ?? (
            <span className="text-fg-4">
              {isArchived
                ? "Prompt not preserved for archived runs."
                : status === "pending"
                  ? "Prompt available after node starts."
                  : "Loading prompt..."}
            </span>
          )}
        </pre>
      )}
    </div>
  );
}

// --- Iter Selector ---

const STATUS_DOTS: Record<NodeStatus, string> = {
  pending: "bg-st-pending",
  running: "bg-st-running",
  awaiting_user: "bg-st-await",
  completed: "bg-st-done",
  failed: "bg-st-failed",
  stopped: "bg-st-stopped",
  stale: "bg-st-stale",
  interrupted: "bg-st-interrupted",
};

function IterSelector({
  iterations,
  selectedIter,
  onSelect,
}: {
  iterations: IterationInfo[];
  selectedIter: number;
  onSelect: (iter: number) => void;
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        className="flex cursor-pointer items-center gap-0.5 rounded px-1 py-0.5 font-mono text-fg-3 transition-colors hover:bg-bg-4 hover:text-fg-2"
        style={{ fontSize: "10px" }}
      >
        iter {selectedIter}
        <ChevronDown size={10} className="text-fg-4" />
      </DropdownMenuTrigger>
      <DropdownMenuContent
        className="min-w-[180px] rounded-md border border-line-strong bg-bg-3 p-1 shadow-lg"
        side="bottom"
        align="start"
      >
        {iterations.map((it) => (
          <DropdownMenuItem
            key={it.iter}
            className={`flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-fg-2 transition-colors hover:bg-bg-4 ${
              it.iter === selectedIter ? "bg-bg-4" : ""
            }`}
            style={{ fontSize: "11px" }}
            onClick={() => onSelect(it.iter)}
            data-testid={`iter-option-${it.iter}`}
          >
            <span
              className={`h-1.5 w-1.5 shrink-0 rounded-full ${STATUS_DOTS[it.status]}`}
            />
            <span className="font-mono">iter {it.iter}</span>
            <span
              className="ml-auto font-mono text-fg-4"
              style={{ fontSize: "10px" }}
            >
              {it.started_at ? formatTime(it.started_at) : ""}
              {it.completed_at ? ` – ${formatTime(it.completed_at)}` : ""}
            </span>
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

// --- IO Section ---

function IOSection({
  title,
  ports,
  runId,
  showFrontmatter,
  onOpenFile,
}: {
  title: string;
  ports: PortIO[];
  runId: string;
  showFrontmatter?: boolean;
  onOpenFile: (portName: string, files: FileInfo[], portType: PortType) => void;
}) {
  return (
    <div className="border-t border-line">
      <div
        className="flex items-center gap-1.5 px-3 py-1.5 text-fg-3"
        style={{ fontSize: "11px" }}
      >
        {title}
        <span className="font-mono text-fg-4" style={{ fontSize: "10px" }}>
          {ports.length}
        </span>
      </div>
      <div className="flex flex-col gap-1 px-3 pb-2">
        {ports.map((port) => (
          <PortRow
            key={port.port}
            port={port}
            runId={runId}
            showFrontmatter={showFrontmatter}
            onOpen={() => onOpenFile(port.port, port.files, port.port_type ?? "markdown")}
          />
        ))}
      </div>
    </div>
  );
}

// --- Port Row ---

const IMAGE_EXTENSIONS = new Set(["png", "jpg", "jpeg", "webp", "gif"]);

function isImageFile(path: string): boolean {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  return IMAGE_EXTENSIONS.has(ext);
}

function PortRow({
  port,
  runId,
  showFrontmatter,
  onOpen,
}: {
  port: PortIO;
  runId: string;
  showFrontmatter?: boolean;
  onOpen: () => void;
}) {
  const firstFile = port.files[0];
  const anyExists = port.files.some((f) => f.exists);
  const portType = port.port_type ?? "markdown";
  // The ordered image list + clicked index currently shown fullscreen in the
  // lightbox, or null when it is closed (#312).
  const [lightbox, setLightbox] = useState<{ images: string[]; index: number } | null>(null);
  const isImage = portType === "image" || portType === "image_list";
  // #333: an html port shows a type badge for parity with image ports (its
  // artifact is otherwise a single markdown-like file row).
  const isHtml = portType === "html";

  let dotClass = "bg-fg-5";
  if (anyExists && port.repeated && port.files.length > 1) {
    dotClass = "bg-st-running";
  } else if (anyExists) {
    dotClass = "bg-st-done";
  }

  let displayPath = firstFile?.path ?? "";
  if (port.files.length > 1 && (port.repeated || isImage)) {
    displayPath = `${port.files.length} files`;
  }

  const totalSize = port.files.reduce((sum, f) => sum + (f.size ?? 0), 0);

  const frontmatter =
    showFrontmatter && !isImage && firstFile?.frontmatter
      ? firstFile.frontmatter
      : null;

  const imageFiles = isImage
    ? port.files.filter((f) => f.exists && isImageFile(f.path))
    : [];

  const gridStyle = {
    gridTemplateColumns: "8px 1fr auto",
    fontSize: "11.5px",
  };

  const children = (
    <>
      {/* Status dot */}
      <div className={`h-2 w-2 rounded-full ${dotClass}`} />

      {/* Name + path */}
      <div className="min-w-0 text-left">
        <div className="flex items-center gap-1.5">
          <span className="font-mono text-fg" style={{ fontSize: "11.5px" }}>
            {port.port}
          </span>
          {port.repeated && (
            <span
              className="rounded border border-line-strong bg-bg-4 px-1 py-px font-mono text-fg-4"
              style={{ fontSize: "9px" }}
            >
              repeated
            </span>
          )}
          {(isImage || isHtml) && (
            <span
              className="rounded border border-line-strong bg-bg-4 px-1 py-px font-mono text-fg-4"
              style={{ fontSize: "9px" }}
              data-testid="port-type-badge"
            >
              {portType}
            </span>
          )}
        </div>
        <div
          className="mt-0.5 truncate font-mono text-fg-3"
          style={{ fontSize: "10.5px" }}
        >
          {displayPath}
        </div>
      </div>

      {/* Meta + arrow icon */}
      <div className="flex items-center gap-2">
        {anyExists && totalSize > 0 && (
          <span className="font-mono text-fg-4" style={{ fontSize: "10px" }}>
            {formatSize(totalSize)}
          </span>
        )}
        {anyExists && (
          <span
            className="font-mono text-fg-3"
            style={{ fontSize: "10.5px" }}
            aria-hidden="true"
          >
            ↗
          </span>
        )}
      </div>

      {/* Image thumbnails */}
      {imageFiles.length > 0 && (
        <div
          className="col-span-3 mt-1 flex gap-1 overflow-x-auto"
          data-testid="image-thumbnails"
        >
          {imageFiles.slice(0, 4).map((f, i) => (
            <img
              key={f.path}
              src={artifactUrl(runId, f.path)}
              alt={f.path.split("/").pop() ?? ""}
              className="h-12 w-12 cursor-zoom-in rounded border border-line object-cover transition-opacity hover:opacity-80"
              onClick={(e) => {
                // Open this thumbnail fullscreen instead of bubbling up to the
                // row button (which opens the artifact modal). Snapshot the
                // FULL imageFiles list (not the .slice(0,4) shown as
                // thumbnails) so arrows reach images behind the +N chip; `i`
                // is a valid index into the full array since the slice starts
                // at 0. A snapshot at click time is immune to poll churn
                // (NodeDetailPanel re-polls IO ~every 1s).
                e.stopPropagation();
                setLightbox({
                  images: imageFiles.map((im) => artifactUrl(runId, im.path)),
                  index: i,
                });
              }}
              data-testid={`thumbnail-${i}`}
            />
          ))}
          {imageFiles.length > 4 && (
            <span
              className="flex h-12 w-12 items-center justify-center rounded border border-line bg-bg-0 font-mono text-fg-4"
              style={{ fontSize: "10px" }}
            >
              +{imageFiles.length - 4}
            </span>
          )}
        </div>
      )}

      {/* Frontmatter card (spans full width below) */}
      {frontmatter && Object.keys(frontmatter).length > 0 && (
        <div
          className="col-span-3 mt-1 grid rounded border border-line bg-bg-0 p-1.5 font-mono"
          style={{
            fontSize: "10px",
            gridTemplateColumns: "auto 1fr",
            gap: "2px 8px",
          }}
        >
          {Object.entries(frontmatter).map(([k, v]) => (
            <FrontmatterKV key={k} field={k} value={v} />
          ))}
        </div>
      )}
    </>
  );

  const lightboxEl = lightbox && (
    <ImageLightbox
      images={lightbox.images}
      index={lightbox.index}
      onClose={() => setLightbox(null)}
    />
  );

  if (anyExists) {
    return (
      <>
        <button
          type="button"
          onClick={onOpen}
          className="port-row grid w-full cursor-pointer items-center gap-2 rounded-md border border-line bg-bg-3 px-2.5 py-2 transition-colors hover:bg-bg-4"
          style={gridStyle}
        >
          {children}
        </button>
        {lightboxEl}
      </>
    );
  }

  return (
    <div
      className="port-row grid items-center gap-2 rounded-md border border-line bg-bg-3 px-2.5 py-2 opacity-60"
      style={gridStyle}
    >
      {children}
      {lightboxEl}
    </div>
  );
}

function FrontmatterKV({ field, value }: { field: string; value: unknown }) {
  const display =
    typeof value === "object" ? JSON.stringify(value) : String(value);
  return (
    <>
      <span className="text-fg-3">{field}</span>
      <span className="text-fg">{display}</span>
    </>
  );
}

function terminalPlaceholder(node: NodeState): string {
  switch (node.status) {
    case "pending":
      return "en attente d’activation";
    case "completed":
      return "Session ended.";
    case "failed":
      return `Failed: ${node.failure_reason ?? "unknown reason"}`;
    case "stopped":
      return `Stopped: ${node.failure_reason ?? "user stopped"}`;
    case "stale":
      return "Agent idle — outputs incomplete";
    case "interrupted":
      // #598 / ADR-0049: the session died on an infra incident, the work is
      // presumed intact — Reopen/Retry re-drives it.
      return `Interrupted: ${node.failure_reason ?? "session died — reopen or retry"}`;
    case "running":
      return "Connecting...";
    case "awaiting_user":
      return "Waiting for user interaction...";
  }
}

function formatTime(iso: string): string {
  try {
    const d = new Date(iso);
    return d.toLocaleTimeString(undefined, {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
  } catch {
    return iso;
  }
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
