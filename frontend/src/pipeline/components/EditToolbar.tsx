import {
  Bot,
  FilePlus,
  Info,
  Maximize,
  Play,
  Plus,
  Redo2,
  RotateCcw,
  SquareTerminal,
  StickyNote,
  Terminal,
  Undo2,
  ZoomIn,
  ZoomOut,
} from "lucide-react";
import { IconButton, Tooltip } from "../../shared/ui/kit";
import { ReviewIcon } from "../../shared/ui/brand/AccentIcons";
import type { PendingTone } from "../../review/model/commentStatus";
import type { RunState } from "../../shared/generated/RunState";
import type { NodeType } from "../../shared/generated/NodeType";
import type { LibraryEntry } from "../model/library";
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
  /** The view group: fit the canvas, zoom in and out. */
  view?: { fit: () => void; zoomIn: () => void; zoomOut: () => void };
}

export default function EditToolbar({ onAddNode, onAddNote, onAddNodeFromYaml, libraryEntries, onLibraryDelete, getDropPosition, infoOpen, onToggleInfo, assistantAvailable = false, assistantActive = false, onOpenAssistant, reviewHref, reviewPending = 0, reviewTone = "pending", reviewTitle = "Review", readOnly = false, finishedRun = false, onReopen, onRetryAll, onOpenShell, run = null, runEdit, view }: Props) {
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
  // A group sits before the trailing controls: they open with a separator.
  const leadingGroup = !readOnly || runEdit != null || finishedRun;
  const sep = <span className="cv-tb__sep" aria-hidden />;

  return (
    <div className="cv-tb" role="toolbar" aria-label="Canvas tools" data-testid="edit-toolbar">
      {/* ADR-0085: the « Edit » control of a Run leads the bar. */}
      {runEdit && (
        <>
          <RunEditControl runEdit={runEdit} />
          {!readOnly && sep}
        </>
      )}

      {/* #315: every editing affordance is suppressed on a read-only canvas. */}
      {!readOnly && (
        <>
          {/* #307: the `+` is a menu — create a Node or a canvas Note, or a
              node from YAML (#345). */}
          <DropdownMenu>
            <Tooltip label="Add">
              <DropdownMenuTrigger data-testid="toolbar-add" className="d-icon" aria-label="Add">
                <Plus size={14} strokeWidth={1.5} aria-hidden />
              </DropdownMenuTrigger>
            </Tooltip>
            <DropdownMenuContent className="cv-menu" side="bottom" align="start">
              <DropdownMenuItem data-testid="add-menu-node" className="cv-menu__item" onClick={() => onAddNode("agent")}>
                <Bot size={14} strokeWidth={1.5} aria-hidden />
                <span>Node</span>
              </DropdownMenuItem>
              <DropdownMenuItem data-testid="add-menu-note" className="cv-menu__item" onClick={() => onAddNote()}>
                <StickyNote size={14} strokeWidth={1.5} aria-hidden />
                <span>Note</span>
              </DropdownMenuItem>
              {/* `FilePlus`, NOT `FileUp` (which belongs to the foreign-workflow import). */}
              <DropdownMenuItem
                data-testid="add-menu-node-from-yaml"
                className="cv-menu__item"
                onClick={() => onAddNodeFromYaml()}
              >
                <FilePlus size={14} strokeWidth={1.5} aria-hidden />
                <span>Node from YAML…</span>
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>

          <LibraryDropdown entries={libraryEntries} onDelete={onLibraryDelete} getDropPosition={getDropPosition} onAddNodeFromYaml={onAddNodeFromYaml} />

          {/* No ForEach add-button: a fan-out is a `collection` loop region
              (#151), created by fanning a selection out over a list field. No
              Merge add-button either (ADR-0082): convergence is drawn by wiring
              several edges into any node. */}
          <IconButton icon={SquareTerminal} label="Script node" data-testid="toolbar-script" onClick={() => onAddNode("script")} />

          {sep}

          <IconButton
            icon={Undo2}
            label={draft?.undoLabel ? `Undo ${draft.undoLabel}` : "Undo"}
            kbd="Ctrl+Z"
            data-testid="toolbar-undo"
            onClick={() => undo()}
            disabled={!canUndo}
          />
          <IconButton icon={Redo2} label="Redo" kbd="Ctrl+Y" data-testid="toolbar-redo" onClick={() => redo()} disabled={!canRedo} />
        </>
      )}

      {/* #598 / ADR-0049: the finished-run group — the three ways to continue
          a TERMINAL, non-archived run. */}
      {finishedRun && (
        <>
          {(!readOnly || runEdit) && sep}
          {onReopen && (
            <IconButton icon={Play} label="Reopen run" data-testid="toolbar-reopen" onClick={onReopen} />
          )}
          {onRetryAll && (
            <IconButton icon={RotateCcw} label="Retry all" data-testid="toolbar-retry-all" onClick={onRetryAll} />
          )}
          {onOpenShell && (
            <IconButton icon={Terminal} label="Open shell in worktree" data-testid="toolbar-open-shell" onClick={onOpenShell} />
          )}
        </>
      )}

      {/* The view group: fit, zoom in, zoom out. */}
      {view && (
        <>
          {leadingGroup && sep}
          <IconButton icon={Maximize} label="Fit view" data-testid="toolbar-fit" onClick={view.fit} />
          <IconButton icon={ZoomIn} label="Zoom in" data-testid="toolbar-zoom-in" onClick={view.zoomIn} />
          <IconButton icon={ZoomOut} label="Zoom out" data-testid="toolbar-zoom-out" onClick={view.zoomOut} />
        </>
      )}

      {run && <RunToolbarActions run={run} readOnly={false} />}

      {(reviewHref || run || onToggleInfo || (assistantAvailable && onOpenAssistant)) && sep}

      {/* #752: the Review quick access — navigation, not a toggle (no
          `aria-pressed`). The pill is the pending count; its tone is the nuance. */}
      {reviewHref && (
        <Tooltip label={reviewTitle}>
          <a
            href={reviewHref}
            data-testid="toolbar-review"
            aria-label={reviewTitle}
            data-pending={reviewPending > 0 ? reviewPending : undefined}
            data-tone={reviewPending > 0 ? reviewTone : undefined}
            className="d-icon"
          >
            <ReviewIcon size={14} />
            {reviewPending > 0 && (
              <span
                data-testid="toolbar-review-pill"
                aria-hidden
                className="cv-tb__badge"
                data-tone={reviewTone}
              >
                {reviewPending}
              </span>
            )}
          </a>
        </Tooltip>
      )}

      {/* The loupe: the search over the Run's transcripts. */}
      {run && <RunSearchToggle runId={run.run_id} />}

      {/* #302 / ADR-0048: the "agent" glyph, immediately left of `(i)`, both
          driving the same Pipeline info panel — the Bot toggles the Assistant
          tab. #938: the host lights at most one of the two. */}
      {assistantAvailable && onOpenAssistant && (
        <IconButton
          icon={Bot}
          label="Pipeline assistant"
          data-testid="toolbar-assistant"
          pressed={assistantActive}
          onClick={onOpenAssistant}
        />
      )}
      {onToggleInfo && (
        <IconButton
          icon={Info}
          label="Pipeline info"
          data-testid="toolbar-info"
          pressed={infoOpen ?? false}
          onClick={onToggleInfo}
        />
      )}
    </div>
  );
}
