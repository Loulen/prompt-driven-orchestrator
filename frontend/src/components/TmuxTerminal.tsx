import { useEffect, useRef, useState, useCallback } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import "@xterm/xterm/css/xterm.css";
import { Maximize2, Minimize2, ExternalLink, Copy } from "lucide-react";
import { Tooltip } from "./ui/tooltip";
import { attachSession, fetchPane } from "../api";
import {
  clipboardKeyAction,
  forcedSelectionEvent,
  isMacPlatform,
  swallowsMotionReport,
  writeClipboardText,
} from "../lib/terminalClipboard";
import { resizeAvoidingAltBufferCorruption } from "../lib/altBufferResize";
import { terminalTheme } from "../lib/terminalTheme";
import { useTheme } from "../hooks/useTheme";

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
  // #759: the live theme. Used to build the palette at mount and to repaint an
  // already-open terminal when the theme switches (xterm paints on a canvas, so
  // it cannot follow a CSS token on its own).
  const { resolved } = useTheme();
  const fitAddonRef = useRef<FitAddon | null>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const [connected, setConnected] = useState(false);
  // #772: drives the toolbar Copy button; xterm's selection lives outside React.
  const [hasSelection, setHasSelection] = useState(false);
  const [copyFeedback, setCopyFeedback] = useState<"copied" | "failed" | null>(null);
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

  // #772: an explicit Copy button for the keyboard-less case (touch, remote
  // desktop) and as the visible hint that selection is browser-side. Uses the
  // execCommand fallback, so it works on a plain-http remote origin too.
  const handleCopy = useCallback(async () => {
    const term = terminalRef.current;
    if (!term || !term.hasSelection()) return;
    const ok = await writeClipboardText(term.getSelection());
    setCopyFeedback(ok ? "copied" : "failed");
    if (ok) term.clearSelection();
    window.setTimeout(() => setCopyFeedback(null), 1500);
  }, []);

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
      theme: terminalTheme(resolved),
      allowTransparency: false,
      scrollback: 5000,
      // #772: on macOS Option+drag is xterm's "force selection while the pty
      // tracks the mouse" modifier only with this on; off it means column select.
      // `forcedSelectionEvent` relies on it.
      macOptionClickForcesSelection: true,
    });

    const fitAddon = new FitAddon();
    const webLinksAddon = new WebLinksAddon();

    term.loadAddon(fitAddon);
    term.loadAddon(webLinksAddon);

    term.open(container);
    fitAddon.fit();

    terminalRef.current = term;
    fitAddonRef.current = fitAddon;

    // #772: copy/paste from the pane. tmux mouse mode (enabled by the daemon so
    // the wheel scrolls tmux scrollback) makes xterm.js report every drag to the
    // pty instead of selecting: the text ends up in tmux's paste buffer on the
    // daemon host, xterm drops tmux's OSC 52 reply, and Ctrl+Shift+C copies "".
    // Rewrite a plain mousedown into the platform's "force selection" form in
    // capture phase, before xterm's own listeners, so the browser owns the drag
    // while the wheel path below stays untouched. Shift/Option+drag already
    // worked and still does.
    const isMac = isMacPlatform();
    const handleMouseDown = (e: MouseEvent) => {
      const clone = forcedSelectionEvent(e, {
        mouseTrackingActive: term.modes.mouseTrackingMode !== "none",
        isMac,
      });
      if (!clone) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      e.target?.dispatchEvent(clone);
    };
    container.addEventListener("mousedown", handleMouseDown, { capture: true });

    // #788: with all-motion tracking xterm reports every buttonless pointer
    // move to the pty and clears its own selection on that "user input", so
    // the selection made above dies as soon as the pointer moves again. Stop
    // such moves in capture phase; drags, clicks and the wheel still go through.
    const handleMouseMove = (e: MouseEvent) => {
      if (
        swallowsMotionReport(e, {
          hasSelection: term.hasSelection(),
          mouseTrackingActive: term.modes.mouseTrackingMode !== "none",
        })
      ) {
        e.stopImmediatePropagation();
      }
    };
    container.addEventListener("mousemove", handleMouseMove, { capture: true });

    // Ctrl+C with a selection copies it (SIGINT otherwise); Ctrl+V pastes like
    // Ctrl+Shift+V instead of sending ^V, which Claude Code reads as "paste
    // image". Returning `false` hands the key to the browser, whose native
    // copy/paste events xterm already serves — no navigator.clipboard, so this
    // works over plain http on a remote daemon.
    term.attachCustomKeyEventHandler((ev) => {
      const action = clipboardKeyAction(ev, {
        hasSelection: term.hasSelection(),
        isMac,
      });
      if (action === "copy") {
        // Let the native copy event read the selection first, then drop it so
        // the next Ctrl+C is a SIGINT again.
        window.setTimeout(() => term.clearSelection(), 0);
        return false;
      }
      if (action === "paste") return false;
      return true;
    });

    const selectionDisposable = term.onSelectionChange(() => {
      setHasSelection(term.hasSelection());
    });

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

    // #771: a live pane in the alternate screen may be in the state xterm's
    // resize corrupts (see `altBufferResize.ts`); the helper resets it first and
    // runs the fit once xterm has processed the reset. A frozen pane has no
    // alternate screen to reset, and nothing to redraw it — plain fit.
    const resizeObserver = new ResizeObserver(() => {
      const apply = () => {
        fitAddon.fit();
        if (ws) sendResize(ws, fitAddon);
      };
      if (!ws) {
        apply();
        return;
      }
      resizeAvoidingAltBufferCorruption(term, fitAddon.proposeDimensions(), apply);
    });
    resizeObserver.observe(container);

    return () => {
      container.removeEventListener("wheel", handleWheel, { capture: true });
      container.removeEventListener("mousedown", handleMouseDown, { capture: true });
      container.removeEventListener("mousemove", handleMouseMove, { capture: true });
      selectionDisposable.dispose();
      setHasSelection(false);
      resizeObserver.disconnect();
      inputDisposable.dispose();
      binaryDisposable.dispose();
      ws?.close();
      term.dispose();
      terminalRef.current = null;
      fitAddonRef.current = null;
      wsRef.current = null;
    };
    // `resolved` is deliberately NOT a dependency: recreating the Terminal on a
    // theme switch would drop the scrollback and the attached socket. The effect
    // below repaints the live instance instead.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [session, mode, frozen]);

  // #759: repaint an already-open terminal when the theme switches.
  useEffect(() => {
    const term = terminalRef.current;
    // `options` is absent when the Terminal is a test double — nothing to repaint.
    if (term?.options) term.options.theme = terminalTheme(resolved);
  }, [resolved]);

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
        {/* #772: copy the browser-side selection; the tooltip doubles as the
            discoverable hint for the keyboard shortcuts. */}
        <Tooltip
          content={
            copyFeedback === "copied"
              ? "Copied"
              : copyFeedback === "failed"
                ? "Copy failed — select the text and press Ctrl+Shift+C"
                : "Copy selection · drag to select, Ctrl+Shift+C / Ctrl+C copies, Ctrl+Shift+V / Ctrl+V pastes"
          }
        >
          <button
            onClick={handleCopy}
            disabled={!hasSelection}
            aria-label="Copy selection"
            className={`flex h-5 w-5 items-center justify-center rounded transition-colors ${
              hasSelection
                ? "cursor-pointer text-fg-3 hover:bg-bg-4 hover:text-fg"
                : "cursor-default text-fg-5"
            }`}
            data-testid="term-copy"
            data-feedback={copyFeedback ?? undefined}
          >
            <Copy size={12} />
          </button>
        </Tooltip>
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
