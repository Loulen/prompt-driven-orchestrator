import { Plus, Info, Undo2, Redo2, SquareTerminal, Box, StickyNote, FilePlus, FileDiff, Bot, Play, RotateCcw, Terminal } from "lucide-react";
import type { PendingTone } from "../../review/model/commentStatus";
import type { RunState } from "../../shared/generated/RunState";
import type { NodeType } from "../../shared/generated/NodeType";
import type { LibraryEntry } from "../model/library";
import { Tooltip } from "../../shared/ui/tooltip";
import LibraryDropdown from "./LibraryDropdown";
import {
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuItem,
} from "../../shared/ui/dropdown-menu";
import { useEditStore } from "../store/editStore";
import { useActiveDraft } from "../store/useDraftPresence";
import RunToolbarActions from "./RunToolbarActions";
import RunEditControl, { type RunEdit } from "./RunEditControl";
import RunSearchToggle from "../../run/components/runSearch/RunSearchToggle";

interface Props {
  onAddNode: (type: NodeType) => void;
  onAddNote: () => void;
  // #345: open the "Add node from YAML…" modal (paste/upload a node definition).
  onAddNodeFromYaml: () => void;
  libraryEntries: LibraryEntry[];
  onLibraryDelete: (name: string) => void;
  getDropPosition?: () => { x: number; y: number };
  // Lights `(i)` (and its `aria-pressed`). #938: on a template canvas the host
  // passes "open on a tab other than Assistant", never with `assistantActive`.
  infoOpen?: boolean;
  onToggleInfo?: () => void;
  // #302 / ADR-0048: the "agent" glyph beside `(i)`. Toggles the Pipeline info
  // panel on the Assistant tab (the library authoring copilot). Only
  // wired for a library *template* canvas (`assistantAvailable`) — on a run
  // canvas the same access path leads to the Manager tab instead.
  assistantAvailable?: boolean;
  assistantActive?: boolean;
  onOpenAssistant?: () => void;
  // #752 (CONTEXT.md « Accès rapide Review »): the Review quick access, in the
  // slot the "Run repositories" toggle used to hold (Repositories is a tab of the
  // Run panel now). A real link to the Run's Review page — same tab, like the
  // Diff tab's "Expand and comment" — with a pill = `pendingCount()` (sent, not
  // resolved). The colour carries the nuance, never the number: outlined blue
  // pending, solid blue ≥ 1 unread reply, amber ≥ 1 resolution proposed (wins).
  // Passed for every non-archived Run; absent on templates and archived Runs.
  reviewHref?: string;
  reviewPending?: number;
  reviewTone?: PendingTone;
  reviewTitle?: string;
  // #315: an archived run's canvas is read-only — hide every editing control
  // (add node/note, library insert, script, undo/redo). Only the
  // Pipeline-info button survives so the archived pipeline stays inspectable.
  // ADR-0085: a Run followed in « pilotage » is read-only the same way.
  readOnly?: boolean;
  // ADR-0085: the « Edit » control of a non-archived Run. Absent on a pipeline
  // tab (always editable) and on an archived Run (read-only for good).
  runEdit?: RunEdit;
  // #598 / ADR-0049: the "finished-run" action group — the three ways to
  // continue a TERMINAL, non-archived run, contextual to the run on screen.
  // Shown only when `finishedRun` is true (terminal ∧ !archived). Variant A
  // (icon cluster): Reopen (Play, accent — re-project & drive), Retry-all
  // (RotateCcw — archive & fresh run, gated by its confirm modal) and Open shell
  // (Terminal — a bash in the worktree). Absent on a live run, where these
  // actions have no meaning.
  finishedRun?: boolean;
  onReopen?: () => void;
  onRetryAll?: () => void;
  onOpenShell?: () => void;
  // #30: the Run of a Run's canvas: the extensions' `run/toolbar` actions sit
  // after the finished-run group. Absent on a template's canvas.
  run?: RunState | null;
}

export default function EditToolbar({ onAddNode, onAddNote, onAddNodeFromYaml, libraryEntries, onLibraryDelete, getDropPosition, infoOpen, onToggleInfo, assistantAvailable = false, assistantActive = false, onOpenAssistant, reviewHref, reviewPending = 0, reviewTone = "pending", reviewTitle = "Review", readOnly = false, finishedRun = false, onReopen, onRetryAll, onOpenShell, run = null, runEdit }: Props) {
  // Read undo/redo straight from the store (ADR-0014 / #226): they have no
  // component-local dependency, unlike the prop-drilled add callbacks, so
  // the point-of-use selector idiom is the right fit. `canUndo`/`canRedo` are
  // derived (reactive) rather than stored — no duplicated state to keep in sync.
  const undo = useEditStore((s) => s.undo);
  const redo = useEditStore((s) => s.redo);
  // ADR-0088: a shared draft keeps the history of my own changes.
  const draft = useActiveDraft();
  const canUndo = useEditStore((s) => {
    const t = s.activeTabId;
    return t != null && (s.history[t]?.past.length ?? 0) > 0;
  }) || (draft?.canUndo ?? false);
  const canRedo = useEditStore((s) => {
    const t = s.activeTabId;
    return t != null && (s.history[t]?.future.length ?? 0) > 0;
  }) || (draft?.canRedo ?? false);
  const undoTip = !draft
    ? "Undo · Ctrl+Z"
    : draft.undoLabel
      ? `Ctrl+Z · Undo your last change: ${draft.undoLabel}`
      : "Ctrl+Z · Undo your last change";
  // A group sits before the trailing controls: they open with a separator.
  const leadingGroup = !readOnly || runEdit != null || finishedRun;

  return (
    <div
      className="absolute left-3 top-3 z-10 flex items-center gap-0.5 rounded-md border border-line bg-bg-2/90 p-1 backdrop-blur-sm shadow-lg"
      data-testid="edit-toolbar"
    >
      {/* #315: every editing affordance is suppressed on a read-only archived
          canvas. Only the Pipeline-info button (below) survives. */}
      {!readOnly && (
        <>
          {/* #307: the `+` is now a dropdown — create a Node (current behaviour)
              or a canvas Note. The trigger keeps `data-testid="toolbar-add"`;
              the sibling script button is unchanged. */}
          <DropdownMenu>
            <DropdownMenuTrigger
              data-testid="toolbar-add"
              className="grid h-7 w-7 cursor-pointer place-items-center rounded text-fg-3 transition-colors hover:bg-bg-4 hover:text-fg active:bg-acc active:text-bg-0 data-[popup-open]:bg-bg-4 data-[popup-open]:text-fg"
              aria-label="Add"
            >
              <Plus size={14} />
            </DropdownMenuTrigger>
            <DropdownMenuContent
              className="min-w-[160px] rounded-md border border-line-strong bg-bg-3 p-1 shadow-lg"
              side="bottom"
              align="start"
            >
              <DropdownMenuItem
                data-testid="add-menu-node"
                className="flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-fg-2 transition-colors hover:bg-bg-4"
                style={{ fontSize: "11.5px" }}
                onClick={() => onAddNode("agent")}
              >
                <Box size={13} className="shrink-0 text-fg-4" />
                <span>Node</span>
              </DropdownMenuItem>
              <DropdownMenuItem
                data-testid="add-menu-note"
                className="flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-fg-2 transition-colors hover:bg-bg-4"
                style={{ fontSize: "11.5px" }}
                onClick={() => onAddNote()}
              >
                <StickyNote size={13} className="shrink-0 text-fg-4" />
                <span>Note</span>
              </DropdownMenuItem>
              {/* #345: 4th way to create a node — from a pasted/uploaded YAML
                  definition. `FilePlus`, NOT `FileUp` (which belongs to the
                  foreign-workflow import). */}
              <DropdownMenuItem
                data-testid="add-menu-node-from-yaml"
                className="flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-fg-2 transition-colors hover:bg-bg-4"
                style={{ fontSize: "11.5px" }}
                onClick={() => onAddNodeFromYaml()}
              >
                <FilePlus size={13} className="shrink-0 text-fg-4" />
                <span>Add node from YAML…</span>
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>

          <span className="mx-0.5 h-4 w-px bg-line" />

          <LibraryDropdown entries={libraryEntries} onDelete={onLibraryDelete} getDropPosition={getDropPosition} onAddNodeFromYaml={onAddNodeFromYaml} />

          {/* No ForEach add-button: a fan-out is a `collection` loop region
              (#151), created by selecting the member(s) and fanning out over a
              list field, not by adding a node. Mirrors the Loop button removal
              (#171). */}

          {/* No Merge add-button either (ADR-0082): convergence is drawn by
              wiring several edges into any node. */}

          <Tooltip content="Script node (deterministic bash)">
            <button
              data-testid="toolbar-script"
              onClick={() => onAddNode("script")}
              className="grid h-7 w-7 cursor-pointer place-items-center rounded text-fg-3 transition-colors hover:bg-bg-4 hover:text-fg active:bg-acc active:text-bg-0"
            >
              <SquareTerminal size={14} />
            </button>
          </Tooltip>

          <span className="mx-0.5 h-4 w-px bg-line" />

          <Tooltip content={undoTip}>
            <button
              data-testid="toolbar-undo"
              onClick={() => undo()}
              disabled={!canUndo}
              className="grid h-7 w-7 place-items-center rounded text-fg-3 transition-colors enabled:cursor-pointer hover:bg-bg-4 hover:text-fg active:bg-acc active:text-bg-0 disabled:cursor-not-allowed disabled:text-fg-5 disabled:hover:bg-transparent disabled:hover:text-fg-5"
            >
              <Undo2 size={14} />
            </button>
          </Tooltip>

          <Tooltip content="Redo · Ctrl+Y">
            <button
              data-testid="toolbar-redo"
              onClick={() => redo()}
              disabled={!canRedo}
              className="grid h-7 w-7 place-items-center rounded text-fg-3 transition-colors enabled:cursor-pointer hover:bg-bg-4 hover:text-fg active:bg-acc active:text-bg-0 disabled:cursor-not-allowed disabled:text-fg-5 disabled:hover:bg-transparent disabled:hover:text-fg-5"
            >
              <Redo2 size={14} />
            </button>
          </Tooltip>
        </>
      )}

      {runEdit && (
        <>
          {!readOnly && <span className="mx-0.5 h-4 w-px bg-line" />}
          <RunEditControl runEdit={runEdit} />
        </>
      )}

      {/* #598 / ADR-0049: the finished-run action group (Variant A). Contextual
          to a TERMINAL, non-archived run — the three ways to continue it. Placed
          between the history group and the view group, in its own cluster. */}
      {finishedRun && (
        <>
          {(!readOnly || runEdit) && <span className="mx-0.5 h-4 w-px bg-line" />}

          {onReopen && (
            <Tooltip content="Reopen — re-project & drive">
              <button
                data-testid="toolbar-reopen"
                aria-label="Reopen run"
                onClick={onReopen}
                className="grid h-7 w-7 cursor-pointer place-items-center rounded text-acc transition-colors hover:bg-bg-4 active:bg-acc active:text-bg-0"
              >
                <Play size={14} />
              </button>
            </Tooltip>
          )}

          {onRetryAll && (
            <Tooltip content="Retry all — archive & fresh run">
              <button
                data-testid="toolbar-retry-all"
                aria-label="Retry all"
                onClick={onRetryAll}
                className="grid h-7 w-7 cursor-pointer place-items-center rounded text-fg-3 transition-colors hover:bg-bg-4 hover:text-fg active:bg-acc active:text-bg-0"
              >
                <RotateCcw size={14} />
              </button>
            </Tooltip>
          )}

          {onOpenShell && (
            <Tooltip content="Open shell in worktree">
              <button
                data-testid="toolbar-open-shell"
                aria-label="Open shell in worktree"
                onClick={onOpenShell}
                className="grid h-7 w-7 cursor-pointer place-items-center rounded text-fg-3 transition-colors hover:bg-bg-4 hover:text-fg active:bg-acc active:text-bg-0"
              >
                <Terminal size={14} />
              </button>
            </Tooltip>
          )}
        </>
      )}

      {run && <RunToolbarActions run={run} readOnly={!leadingGroup} />}

      {/* #752: the Review quick access — navigation, not a toggle (no
          `aria-pressed`). The pill is the pending count; its tone is the nuance. */}
      {reviewHref && (
        <>
          {leadingGroup && <span className="mx-0.5 h-4 w-px bg-line" />}

          <Tooltip content={reviewTitle}>
            <a
              href={reviewHref}
              data-testid="toolbar-review"
              aria-label={reviewTitle}
              data-pending={reviewPending > 0 ? reviewPending : undefined}
              data-tone={reviewPending > 0 ? reviewTone : undefined}
              className="relative grid h-7 w-7 cursor-pointer place-items-center rounded text-fg-3 transition-colors hover:bg-bg-4 hover:text-fg active:bg-acc active:text-bg-0"
            >
              <FileDiff size={14} />
              {reviewPending > 0 && (
                <span
                  data-testid="toolbar-review-pill"
                  aria-hidden
                  className={`absolute -right-[5px] -top-1 box-border grid h-[15px] min-w-[15px] place-items-center rounded-[8px] border-2 border-bg-2 px-1 font-sans font-semibold leading-none ${
                    reviewTone === "proposed"
                      ? "bg-st-await text-on-acc-warn"
                      : reviewTone === "unread"
                        ? "bg-st-running text-white"
                        : "bg-st-running-bg text-st-running shadow-[inset_0_0_0_1px_rgba(59,130,246,.35)]"
                  }`}
                  style={{ fontSize: "9.5px" }}
                >
                  {reviewPending}
                </span>
              )}
            </a>
          </Tooltip>
        </>
      )}

      {/* The loupe: the search over the Run's transcripts, between Review and
          the info group. */}
      {run && (
        <>
          {leadingGroup && !reviewHref && <span className="mx-0.5 h-4 w-px bg-line" />}
          <RunSearchToggle runId={run.run_id} />
        </>
      )}

      {(onToggleInfo || (assistantAvailable && onOpenAssistant)) && (
        <>
          {/* #315: no leading separator when the info group is the sole control. */}
          {leadingGroup && !reviewHref && !run && <span className="mx-0.5 h-4 w-px bg-line" />}

          {/* #302 / ADR-0048: the "agent" glyph, immediately left of `(i)`, both
              driving the same Pipeline info panel — the Bot toggles the Assistant
              tab. Grouped with `(i)`, so no separator between them. #938: the host
              lights at most one of the two, from the tab actually shown. */}
          {assistantAvailable && onOpenAssistant && (
            <Tooltip content="Pipeline assistant">
              <button
                data-testid="toolbar-assistant"
                aria-label="Pipeline assistant"
                aria-pressed={assistantActive}
                onClick={onOpenAssistant}
                className={`grid h-7 w-7 cursor-pointer place-items-center rounded transition-colors ${
                  assistantActive
                    ? "bg-acc text-bg-0"
                    : "text-fg-3 hover:bg-bg-4 hover:text-fg active:bg-acc active:text-bg-0"
                }`}
              >
                <Bot size={14} />
              </button>
            </Tooltip>
          )}

          {onToggleInfo && (
            <Tooltip content="Pipeline info">
              <button
                data-testid="toolbar-info"
                aria-label="Pipeline info"
                aria-pressed={infoOpen ?? false}
                onClick={onToggleInfo}
                className={`grid h-7 w-7 cursor-pointer place-items-center rounded transition-colors ${
                  infoOpen
                    ? "bg-acc text-bg-0"
                    : "text-fg-3 hover:bg-bg-4 hover:text-fg active:bg-acc active:text-bg-0"
                }`}
              >
                <Info size={14} />
              </button>
            </Tooltip>
          )}
        </>
      )}
    </div>
  );
}
