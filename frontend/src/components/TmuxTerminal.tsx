import { useEffect, useRef, useState, useCallback } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import "@xterm/xterm/css/xterm.css";
import { Maximize2, Minimize2, ExternalLink } from "lucide-react";
import { Tooltip } from "./ui/tooltip";
import { attachSession, fetchPane } from "../api";

/** Which node iteration's frozen pane to read when the live session is gone (#617). */
export interface PaneSource {
  runId: string;
  nodeId: string;
  iter: number;
}

interface Props {
  session: string;
  expanded?: boolean;
  onExpand?: () => void;
  status?: string;
  /** #617: where to read the post-mortem pane once the node's iteration is
   *  terminal. Omit ⇒ live attach only (the Run shell has no node identity). */
  paneSource?: PaneSource;
}

// A node iteration in one of these states has had its tmux session reaped on the
// terminal transition (#205, the one-live-iteration invariant), so attaching a PTY
// to it can only produce tmux's `can't find session:` on a dead socket. What
// survives is the snapshot the daemon froze on the way out — and until #617 the UI
// never asked for it, so the primary surface of every finished node was that error
// string. The set mirrors the daemon's own `iter_is_terminal` in `node_pane`:
// `interrupted` is absent from both, its session may still be alive.
const REAPED_STATUSES = new Set(["completed", "failed", "stopped", "stale"]);

// xterm writes raw bytes: a snapshot captured with `tmux capture-pane -pe` is
// newline-separated, and a bare \n moves down without returning, so every line
// would start where the previous one ended. Normalise to CRLF without doubling
// the \r of a line that already carries one.
function toTerminalNewlines(content: string): string {
  return content.replace(/\r?\n/g, "\r\n");
}

// Send a resize message to the daemon, but only if the dimensions are valid.
// FitAddon.proposeDimensions() can momentarily return 0-rows/0-cols during
// a transient layout pass (container attached but not yet measured). The
// daemon's resize decoder rejects zero values, and historically would treat
// the rejected JSON as user input — injecting stray characters into whatever
// has focus in tmux. Guarding here closes that hole at the source.
function sendResize(ws: WebSocket, fitAddon: FitAddon): void {
  if (ws.readyState !== WebSocket.OPEN) return;
  const dims = fitAddon.proposeDimensions();
  if (!dims) return;
  if (!Number.isFinite(dims.cols) || !Number.isFinite(dims.rows)) return;
  if (dims.cols <= 0 || dims.rows <= 0) return;
  ws.send(
    JSON.stringify({ type: "resize", cols: dims.cols, rows: dims.rows }),
  );
}

// What this terminal is showing. `probing` is the beat before the daemon has said
// whether the reaped iteration left a snapshot behind; `live` is the PTY attach.
type PaneMode = "probing" | "live" | "frozen";

export default function TmuxTerminal({
  session,
  expanded = false,
  onExpand,
  status,
  paneSource,
}: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const [connected, setConnected] = useState(false);
  const [frozen, setFrozen] = useState<{
    content: string;
    /** `false` ⇒ the daemon has no snapshot either; say so instead of pretending. */
    preserved: boolean;
  } | null>(null);

  const reaped =
    paneSource !== undefined &&
    status !== undefined &&
    REAPED_STATUSES.has(status);

  // Decided **once per session identity**, not on every status change: a node that
  // settles under the user's eyes keeps the live buffer it already has (the daemon
  // may not even have frozen the snapshot yet), while a node opened after it
  // settled reads the snapshot. A retry spawns a new iteration — a new session name
  // — so the live path is re-entered there, which is what this re-decision is for.
  const [mode, setMode] = useState<PaneMode>(reaped ? "probing" : "live");
  const [decidedFor, setDecidedFor] = useState(session);
  if (decidedFor !== session) {
    setDecidedFor(session);
    setMode(reaped ? "probing" : "live");
    setFrozen(null);
  }

  const handleDetach = useCallback(async () => {
    try {
      await attachSession(session);
    } catch (e) {
      console.error("Failed to detach terminal:", e);
    }
  }, [session]);

  // #617: read the frozen pane before opening any socket. `GET …/pane` answers
  // `live` when the session is somehow still up (then we attach as usual) and
  // `snapshot` for the reaped-and-frozen case this exists for. It never resurrects
  // a terminal iteration, so asking is free of side effects — which is why the
  // probe is gated on a reaped status rather than run for every node.
  // Deps are the three primitives, not the `paneSource` object: the detail panel
  // re-renders on every I/O poll tick, so an object identity here would cancel and
  // restart the probe once a second and never land.
  const paneRunId = paneSource?.runId;
  const paneNodeId = paneSource?.nodeId;
  const paneIter = paneSource?.iter;
  useEffect(() => {
    if (mode !== "probing") return;
    if (paneRunId === undefined || paneNodeId === undefined || paneIter === undefined) {
      return;
    }
    let cancelled = false;
    fetchPane(paneRunId, paneNodeId, paneIter)
      .then((pane) => {
        if (cancelled) return;
        if (pane.source === "live" || pane.source === "resumed") {
          setMode("live");
          return;
        }
        setFrozen({
          content: pane.content,
          preserved: pane.source === "snapshot",
        });
        setMode("frozen");
      })
      .catch(() => {
        if (cancelled) return;
        setFrozen({ content: "Pane unavailable.", preserved: false });
        setMode("frozen");
      });
    return () => {
      cancelled = true;
    };
  }, [mode, paneRunId, paneNodeId, paneIter]);

  useEffect(() => {
    if (!containerRef.current) return;
    if (mode === "probing") return;
    const container = containerRef.current;

    const isFrozen = mode === "frozen";

    const term = new Terminal({
      // A frozen pane has no cursor to blink — the session it belonged to is gone.
      cursorBlink: !isFrozen,
      // Nothing to type into: the socket is not opened at all below.
      disableStdin: isFrozen,
      fontSize: 11,
      fontFamily: "'Geist Mono Variable', monospace",
      theme: {
        background: "#0f1115",
        foreground: "#e6e8eb",
        cursor: "#10b981",
        selectionBackground: "#2a2d35",
        black: "#0f1115",
        red: "#ef4444",
        green: "#10b981",
        yellow: "#f59e0b",
        blue: "#3b82f6",
        magenta: "#8b5cf6",
        cyan: "#06b6d4",
        white: "#e6e8eb",
        brightBlack: "#5a6270",
        brightRed: "#f87171",
        brightGreen: "#34d399",
        brightYellow: "#fbbf24",
        brightBlue: "#60a5fa",
        brightMagenta: "#a78bfa",
        brightCyan: "#22d3ee",
        brightWhite: "#f8fafc",
      },
      allowTransparency: false,
      scrollback: 5000,
    });

    const fitAddon = new FitAddon();
    const webLinksAddon = new WebLinksAddon();

    term.loadAddon(fitAddon);
    term.loadAddon(webLinksAddon);

    term.open(container);
    fitAddon.fit();

    terminalRef.current = term;
    fitAddonRef.current = fitAddon;

    // #617: a frozen pane opens **no** socket. Attaching a PTY to a session the
    // daemon already reaped is what put tmux's `can't find session:` on the primary
    // surface of every finished node; the snapshot is written straight into the same
    // xterm instead, so scrollback, colours and selection all keep working.
    const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
    const wsUrl = `${protocol}//${window.location.host}/sessions/${encodeURIComponent(session)}/pty`;
    const ws = isFrozen ? null : new WebSocket(wsUrl);
    if (ws) {
      ws.binaryType = "arraybuffer";
    }
    wsRef.current = ws;

    if (isFrozen) {
      term.write(toTerminalNewlines(frozen?.content ?? ""));
    }

    ws?.addEventListener("open", () => {
      setConnected(true);
      sendResize(ws, fitAddon);
    });

    ws?.addEventListener("message", (event) => {
      if (event.data instanceof ArrayBuffer) {
        term.write(new Uint8Array(event.data));
      } else if (typeof event.data === "string") {
        term.write(event.data);
      }
    });

    ws?.addEventListener("close", () => {
      setConnected(false);
    });

    ws?.addEventListener("error", () => {
      setConnected(false);
    });

    const inputDisposable = term.onData((data) => {
      if (ws && ws.readyState === WebSocket.OPEN) {
        const encoder = new TextEncoder();
        ws.send(encoder.encode(data));
      }
    });

    const binaryDisposable = term.onBinary((data) => {
      if (ws && ws.readyState === WebSocket.OPEN) {
        const buffer = new Uint8Array(data.length);
        for (let i = 0; i < data.length; i++) {
          buffer[i] = data.charCodeAt(i);
        }
        ws.send(buffer);
      }
    });

    // xterm.js's own viewport handler translates wheel into Application-Cursor
    // arrow-key escapes (ESC O A / ESC O B) when the inner buffer is in
    // alt-screen mode + DECCKM — which is the normal case for any TUI we host
    // (Claude Code, vim, less, etc.). Real wheel events fire on .xterm-screen
    // deep inside the container, so a bubble-phase listener here would arrive
    // *after* xterm's handler has already pushed those bytes to the WS. We
    // register in capture phase so we run first and can stopImmediatePropagation
    // before xterm's handler sees the event.
    //
    // However, when tmux has mouse mode enabled, it requests mouse tracking
    // from the terminal. In that mode xterm.js correctly encodes wheel events
    // as mouse-report escape sequences (not arrow keys). We must let those
    // through so tmux can enter copy-mode and scroll its own scrollback.
    const handleWheel = (e: WheelEvent) => {
      if (e.ctrlKey || e.shiftKey || e.metaKey) return;
      if (term.modes.mouseTrackingMode !== "none") return;
      e.preventDefault();
      e.stopImmediatePropagation();
      if (term.buffer.active.type === "alternate") return;
      const lines = Math.round(e.deltaY / 25) || (e.deltaY > 0 ? 1 : -1);
      term.scrollLines(lines);
    };
    container.addEventListener("wheel", handleWheel, {
      passive: false,
      capture: true,
    });

    const resizeObserver = new ResizeObserver(() => {
      fitAddon.fit();
      if (ws) sendResize(ws, fitAddon);
    });
    resizeObserver.observe(container);

    return () => {
      container.removeEventListener("wheel", handleWheel, { capture: true });
      resizeObserver.disconnect();
      inputDisposable.dispose();
      binaryDisposable.dispose();
      ws?.close();
      term.dispose();
      terminalRef.current = null;
      fitAddonRef.current = null;
      wsRef.current = null;
    };
  }, [session, mode, frozen]);

  const isActive =
    status === "running" || status === "awaiting_user" || status === "stale";

  let dotClass: string;
  let statusLabel: string;
  if (mode === "probing") {
    dotClass = "bg-fg-5";
    statusLabel = "reading pane…";
  } else if (mode === "frozen") {
    // Named, not dressed up as a connection: what is on screen is the pane PDO
    // froze when it reaped the session, and the reap is the invariant, not a fault.
    dotClass = "bg-fg-5";
    statusLabel = frozen?.preserved ? "snapshot · session reaped" : "no pane kept";
  } else if (!connected) {
    dotClass = "bg-fg-5";
    statusLabel = "disconnected";
  } else if (isActive) {
    dotClass = "animate-pulse bg-st-running";
    statusLabel = "attached · live";
  } else {
    dotClass = "bg-st-done";
    statusLabel = "connected";
  }

  return (
    <div
      className="flex flex-1 flex-col overflow-hidden"
      data-testid="tmux-terminal"
    >
      {/* Toolbar */}
      <div
        className="flex items-center gap-1.5 border-b border-line px-3 py-1.5 text-fg-3"
        style={{ fontSize: "11px" }}
        data-testid="term-toolbar"
      >
        <span className={`h-1.5 w-1.5 rounded-full ${dotClass}`} />
        <span className="font-mono text-fg-4" style={{ fontSize: "10px" }}>
          {session}
        </span>
        <span
          className={`rounded border px-1 py-px font-mono ${
            connected
              ? "border-st-done/30 text-st-done"
              : "border-line-strong text-fg-4"
          }`}
          style={{ fontSize: "9px" }}
        >
          {statusLabel}
        </span>
        <span className="flex-1" />
        {onExpand && (
          <Tooltip
            content={
              expanded ? "Collapse terminal" : "Expand terminal"
            }
          >
            <button
              onClick={onExpand}
              className="flex h-5 w-5 cursor-pointer items-center justify-center rounded text-fg-3 transition-colors hover:bg-bg-4 hover:text-fg"
              data-testid="term-expand"
            >
              {expanded ? (
                <Minimize2 size={12} />
              ) : (
                <Maximize2 size={12} />
              )}
            </button>
          </Tooltip>
        )}
        {/* No session to attach to once the pane is frozen — offering the button
            would be an action that can only fail. */}
        {mode !== "frozen" && (
          <Tooltip content="Detach to OS terminal">
            <button
              onClick={handleDetach}
              className="flex h-5 w-5 cursor-pointer items-center justify-center rounded text-fg-3 transition-colors hover:bg-bg-4 hover:text-fg"
              data-testid="term-detach"
            >
              <ExternalLink size={12} />
            </button>
          </Tooltip>
        )}
      </div>

      {/* Terminal container */}
      <div
        ref={containerRef}
        className="min-h-0 flex-1 bg-bg-0"
        data-testid="xterm-container"
      />
    </div>
  );
}
