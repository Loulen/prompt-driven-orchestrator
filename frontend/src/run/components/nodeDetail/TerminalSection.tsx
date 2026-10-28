import { useCallback, useState, type Dispatch, type ReactNode, type SetStateAction } from "react";
import { Maximize2 } from "lucide-react";
import type { NodeState } from "../../../shared/generated/NodeState";
import type { NodeStatus } from "../../../shared/generated/NodeStatus";
import {
  ResizablePanelGroup,
  ResizablePanel,
  ResizableHandle,
} from "../../../shared/ui/resizable";
import TmuxTerminal from "../../../session/components/TmuxTerminal";
import { terminalPlaceholder } from "../../model/nodeStatus";
import { importBlockedReason, SEND_NEEDS_CONTROL } from "../../model/nodeImport";
import ImportFilesModal from "./ImportFilesModal";
import { STATUS_BORDER } from "../../../nodeStyles";
import type { TerminalView } from "./types";
import {
  EnlargedCompletionGestures,
  EnlargedTerminalIdentity,
  GestureVerdictPopover,
  type TerminalCompletion,
} from "./EnlargedTerminalBar";
import { useEnlargedTerminal } from "./useEnlargedTerminal";

/** #44: the enlarged terminal's drop shadow and backdrop, `RunShellModal`'s. */
const OVERLAY_SHADOW = "0 30px 80px rgba(0,0,0,0.6)";
const OVERLAY_BACKDROP = { background: "rgba(5,7,10,0.66)", backdropFilter: "blur(4px)" } as const;
/** #44: the light halo an awaiting frame adds, at 18% of the status colour. */
const AWAIT_HALO = "0 0 0 3px color-mix(in srgb, var(--color-st-await) 18%, transparent)";

/**
 * The terminal inset and the details pane below it, laid out per `terminalView`
 * (#346): minimized bar + full-height details, split, or the « Terminal
 * agrandi » (#44): the same terminal lifted over the whole app.
 */
export default function TerminalSection({
  node,
  nodeName,
  runId,
  isArchived = false,
  selectedIter,
  selectedIterStatus,
  sessionName,
  showTerminal,
  terminalView,
  setTerminalView,
  setTerminalLive,
  setTerminalSpectating,
  terminalSpectating = false,
  detailsPane,
  completion,
}: {
  node: NodeState;
  nodeName?: string | null;
  runId: string;
  isArchived?: boolean;
  selectedIter: number;
  selectedIterStatus: NodeStatus;
  sessionName: string;
  showTerminal: boolean;
  terminalView: TerminalView;
  setTerminalView: Dispatch<SetStateAction<TerminalView>>;
  setTerminalLive: (live: boolean) => void;
  setTerminalSpectating: (spectating: boolean) => void;
  /** This browser only watches the terminal (ADR-0075): it may import, not
   *  write into the agent's input. */
  terminalSpectating?: boolean;
  detailsPane: ReactNode;
  completion: TerminalCompletion;
}) {
  // #44: the « Terminal agrandi ». The terminal's own wrapper switches to
  // `position: fixed` (same element, same React parent, no portal), so the
  // xterm instance and its WebSocket survive the round trip; the existing
  // ResizeObserver resizes the grid both ways.
  const expanded = terminalView === "expanded";
  const collapse = useCallback(() => setTerminalView("split"), [setTerminalView]);
  // « Fichier importé en cours de Run »: the files dropped on the terminal (or
  // `[]` from its Import button) while the import modal is open.
  const [importFiles, setImportFiles] = useState<File[] | null>(null);
  const importReason = importBlockedReason({
    isArchived,
    selectedIter,
    currentIter: node.iter,
    status: selectedIterStatus,
  });
  const {
    frameRef,
    popoverRef,
    gesturePopover,
    setGesturePopover,
    onFramePointerDownCapture,
    onBackdropPointerDown,
    onBackdropPointerUp,
  } = useEnlargedTerminal(expanded, collapse, importFiles !== null);
  const closeImport = useCallback(() => {
    setImportFiles(null);
    // Back to the agent's input, where a sent text waits for Enter.
    frameRef.current
      ?.querySelector<HTMLTextAreaElement>(".xterm-helper-textarea")
      ?.focus();
  }, [frameRef]);

  // One element in both modes (only its classes change), so the terminal under
  // it is never remounted. Fixed positioning needs no ancestor with a
  // `transform` / `filter` / `contain` (none today).
  const terminalPane = (
    <div
      ref={frameRef}
      className={
        expanded
          ? `fixed inset-4 z-[41] flex flex-col overflow-hidden rounded-lg border-[1.5px] bg-surface-1 transition-[border-color,box-shadow] duration-[250ms] ${STATUS_BORDER[node.status]}`
          : "flex h-full flex-col overflow-hidden"
      }
      style={
        expanded
          ? {
              boxShadow:
                node.status === "awaiting_user"
                  ? `${AWAIT_HALO}, ${OVERLAY_SHADOW}`
                  : OVERLAY_SHADOW,
            }
          : undefined
      }
      data-testid={expanded ? "terminal-fullsize" : "terminal-pane-wrapper"}
      data-status={expanded ? node.status : undefined}
      role={expanded ? "dialog" : undefined}
      aria-modal={expanded ? true : undefined}
      aria-label={expanded ? `Terminal of ${nodeName ?? node.node_id}` : undefined}
      onPointerDownCapture={onFramePointerDownCapture}
    >
      {showTerminal ? (
        <TmuxTerminal
          session={sessionName}
          expanded={expanded}
          onExpand={() =>
            setTerminalView((v) => (v === "expanded" ? "split" : "expanded"))
          }
          toolbarIdentity={
            expanded ? (
              <EnlargedTerminalIdentity node={node} nodeName={nodeName} iter={selectedIter} />
            ) : undefined
          }
          toolbarActions={
            expanded && completion.showGestures ? (
              <EnlargedCompletionGestures
                completion={completion}
                onGesture={setGesturePopover}
              />
            ) : undefined
          }
          status={selectedIterStatus}
          // #617: the iteration this panel is showing, so a terminal one can
          // read back the pane PDO froze when it reaped the session. The
          // selected iter — not `node.iter` — because the IterSelector can be
          // sitting on an older, long-reaped iteration.
          paneSource={{ runId, nodeId: node.node_id, iter: selectedIter }}
          onLiveSocketChange={setTerminalLive}
          onSpectatingChange={setTerminalSpectating}
          onImportFiles={isArchived ? undefined : setImportFiles}
          importDisabledReason={importReason}
        />
      ) : (
        <div className="flex h-full flex-col" data-testid="pending-placeholder">
          <div
            className="flex items-center gap-1.5 border-b border-line px-3 py-1.5 text-text-3"
            style={{ fontSize: "11px" }}
          >
            <span className="h-1.5 w-1.5 rounded-full bg-text-3" />
            Terminal
          </div>
          <div className="flex flex-1 items-center justify-center bg-surface-0">
            <span className="text-text-3" style={{ fontSize: "11px" }}>
              {terminalPlaceholder(node)}
            </span>
          </div>
        </div>
      )}
      {expanded && (
        <GestureVerdictPopover
          gesture={gesturePopover}
          completion={completion}
          iter={selectedIter}
          popoverRef={popoverRef}
        />
      )}
      {importFiles !== null && (
        <ImportFilesModal
          runId={runId}
          nodeId={node.node_id}
          iter={node.iter}
          session={sessionName}
          initialFiles={importFiles}
          blockedReason={importReason}
          sendBlockedReason={terminalSpectating ? SEND_NEEDS_CONTROL : null}
          onClose={closeImport}
        />
      )}
    </div>
  );

  // #44: the dimmed backdrop, a sibling under the frame. Not interactive beyond
  // closing: press and release must both land on it.
  const backdrop = expanded ? (
    <div
      className="fixed inset-0 z-40"
      style={OVERLAY_BACKDROP}
      data-testid="terminal-overlay-backdrop"
      onPointerDown={onBackdropPointerDown}
      onPointerUp={onBackdropPointerUp}
    />
  ) : null;

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
          className="flex items-center gap-1.5 border-b border-line px-3 py-1.5 text-text-3 transition-colors hover:text-text-2"
          style={{ fontSize: "11px" }}
          title="Agrandir le terminal"
        >
          <span className="h-1.5 w-1.5 rounded-full bg-text-3" />
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

  // Keep `TmuxTerminal` mounted across the enlarge toggle (#44): the split
  // layout stays as it is underneath, and only the terminal's own wrapper lifts
  // out of it (`position: fixed`). The tree is identical in both modes (the
  // backdrop's slot is merely empty in split), so the WebSocket and xterm
  // instance survive the toggle.
  return (
    <ResizablePanelGroup
      orientation="vertical"
      className="min-h-0 flex-1"
      // The overlay lives inside this group: without this, a drag across the
      // hidden separator would resize the split underneath.
      disabled={expanded}
    >
      <ResizablePanel id="terminal" defaultSize={45} minSize="100px">
        {backdrop}
        {terminalPane}
      </ResizablePanel>
      <ResizableHandle />
      <ResizablePanel id="details" defaultSize={55} minSize="100px">
        {detailsPane}
      </ResizablePanel>
    </ResizablePanelGroup>
  );
}
