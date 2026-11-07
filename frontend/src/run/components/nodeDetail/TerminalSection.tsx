import { useCallback, useState, type Dispatch, type ReactNode, type SetStateAction } from "react";
import { ChevronDown, Lock } from "lucide-react";
import { nodeStatusTone } from "../../../shared/ui/kit/statusMeta";
import { toneColor } from "../../../shared/ui/kit/tones";
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
import type { TerminalView } from "./types";
import {
  EnlargedTerminalIdentity,
  GestureVerdictPopover,
  type TerminalCompletion,
} from "./EnlargedTerminalBar";
import { useEnlargedTerminal } from "./useEnlargedTerminal";

/** #44: the enlarged terminal's drop shadow and backdrop. */
const OVERLAY_SHADOW = "0 30px 80px var(--shadow)";
const OVERLAY_BACKDROP = { background: "var(--dim)", backdropFilter: "blur(4px)" } as const;
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
          ? "fixed inset-4 z-[41] flex flex-col overflow-hidden rounded border transition-[border-color,box-shadow] duration-[250ms]"
          : "flex h-full flex-col overflow-hidden"
      }
      style={
        expanded
          ? {
              borderColor: `color-mix(in oklab, ${toneColor(nodeStatusTone(node.status))} 50%, var(--line))`,
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
            expanded && completion.show ? completion.renderApprove(setGesturePopover) : undefined
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
        <div className="flex h-full flex-col" data-mode="dark" data-testid="pending-placeholder">
          <div className="ri-term-bar">
            <Lock size={12} strokeWidth={1.5} aria-hidden />
            <span className="ri-term-bar__session">no session</span>
          </div>
          <div className="ri-term-empty">{terminalPlaceholder(node)}</div>
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
          data-mode="dark"
          data-testid="term-restore"
          className="ri-term-min"
          title="Show the terminal"
        >
          <Lock size={12} strokeWidth={1.5} aria-hidden />
          <span className="truncate font-mono">{sessionName}</span>
          <span className="ri-term-min__fill" />
          <span className="ri-term-bar__tag">ended</span>
          <ChevronDown size={12} strokeWidth={1.5} aria-hidden />
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
