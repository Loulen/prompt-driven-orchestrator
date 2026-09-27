import type { Dispatch, ReactNode, SetStateAction } from "react";
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
import type { TerminalView } from "./types";

/**
 * The terminal inset and the details pane below it, laid out per `terminalView`
 * (#346): minimized bar + full-height details, split, or expanded terminal.
 */
export default function TerminalSection({
  node,
  runId,
  selectedIter,
  selectedIterStatus,
  sessionName,
  showTerminal,
  terminalView,
  setTerminalView,
  setTerminalLive,
  setTerminalSpectating,
  detailsPane,
}: {
  node: NodeState;
  runId: string;
  selectedIter: number;
  selectedIterStatus: NodeStatus;
  sessionName: string;
  showTerminal: boolean;
  terminalView: TerminalView;
  setTerminalView: Dispatch<SetStateAction<TerminalView>>;
  setTerminalLive: (live: boolean) => void;
  setTerminalSpectating: (spectating: boolean) => void;
  detailsPane: ReactNode;
}) {
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
          status={selectedIterStatus}
          // #617: the iteration this panel is showing, so a terminal one can
          // read back the pane PDO froze when it reaped the session. The
          // selected iter — not `node.iter` — because the IterSelector can be
          // sitting on an older, long-reaped iteration.
          paneSource={{ runId, nodeId: node.node_id, iter: selectedIter }}
          onLiveSocketChange={setTerminalLive}
          onSpectatingChange={setTerminalSpectating}
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
}
