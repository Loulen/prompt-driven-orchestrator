import { useEffect, useRef, useState, useCallback } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import "@xterm/xterm/css/xterm.css";
import { Maximize2, Minimize2, ExternalLink } from "lucide-react";
import { Tooltip } from "./ui/tooltip";
import { attachSession } from "../api";

interface Props {
  session: string;
  expanded?: boolean;
  onExpand?: () => void;
  status?: string;
}

export default function TmuxTerminal({
  session,
  expanded = false,
  onExpand,
  status,
}: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const [connected, setConnected] = useState(false);

  const handleDetach = useCallback(async () => {
    try {
      await attachSession(session);
    } catch (e) {
      console.error("Failed to detach terminal:", e);
    }
  }, [session]);

  useEffect(() => {
    if (!containerRef.current) return;
    const container = containerRef.current;

    const term = new Terminal({
      cursorBlink: true,
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

    // WebSocket connection
    const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
    const wsUrl = `${protocol}//${window.location.host}/sessions/${encodeURIComponent(session)}/pty`;
    const ws = new WebSocket(wsUrl);
    ws.binaryType = "arraybuffer";
    wsRef.current = ws;

    ws.addEventListener("open", () => {
      setConnected(true);
      // Send initial resize
      const dims = fitAddon.proposeDimensions();
      if (dims) {
        ws.send(
          JSON.stringify({ type: "resize", cols: dims.cols, rows: dims.rows }),
        );
      }
    });

    ws.addEventListener("message", (event) => {
      if (event.data instanceof ArrayBuffer) {
        term.write(new Uint8Array(event.data));
      } else if (typeof event.data === "string") {
        term.write(event.data);
      }
    });

    ws.addEventListener("close", () => {
      setConnected(false);
    });

    ws.addEventListener("error", () => {
      setConnected(false);
    });

    // Forward user input to WS
    const inputDisposable = term.onData((data) => {
      if (ws.readyState === WebSocket.OPEN) {
        const encoder = new TextEncoder();
        ws.send(encoder.encode(data));
      }
    });

    // Handle binary input (for paste etc.)
    const binaryDisposable = term.onBinary((data) => {
      if (ws.readyState === WebSocket.OPEN) {
        const buffer = new Uint8Array(data.length);
        for (let i = 0; i < data.length; i++) {
          buffer[i] = data.charCodeAt(i);
        }
        ws.send(buffer);
      }
    });

    // xterm.js in alt-screen mode forwards wheel as arrow-key escapes to the TTY.
    const handleWheel = (e: WheelEvent) => {
      if (e.ctrlKey || e.shiftKey || e.metaKey) return;
      e.preventDefault();
      e.stopPropagation();
      const lines = Math.round(e.deltaY / 25) || (e.deltaY > 0 ? 1 : -1);
      term.scrollLines(lines);
    };
    container.addEventListener("wheel", handleWheel, { passive: false });

    // Resize observer
    const resizeObserver = new ResizeObserver(() => {
      fitAddon.fit();
      if (ws.readyState === WebSocket.OPEN) {
        const dims = fitAddon.proposeDimensions();
        if (dims) {
          ws.send(
            JSON.stringify({
              type: "resize",
              cols: dims.cols,
              rows: dims.rows,
            }),
          );
        }
      }
    });
    resizeObserver.observe(container);

    return () => {
      container.removeEventListener("wheel", handleWheel);
      resizeObserver.disconnect();
      inputDisposable.dispose();
      binaryDisposable.dispose();
      ws.close();
      term.dispose();
      terminalRef.current = null;
      fitAddonRef.current = null;
      wsRef.current = null;
    };
  }, [session]);

  const isActive =
    status === "running" || status === "awaiting_user";

  let dotClass: string;
  let statusLabel: string;
  if (!connected) {
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
      className={`flex flex-col overflow-hidden ${expanded ? "flex-1" : ""}`}
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
        <Tooltip content="Detach to OS terminal">
          <button
            onClick={handleDetach}
            className="flex h-5 w-5 cursor-pointer items-center justify-center rounded text-fg-3 transition-colors hover:bg-bg-4 hover:text-fg"
            data-testid="term-detach"
          >
            <ExternalLink size={12} />
          </button>
        </Tooltip>
      </div>

      {/* Terminal container */}
      <div
        ref={containerRef}
        className={`min-h-0 bg-bg-0 ${expanded ? "flex-1" : ""}`}
        style={!expanded ? { height: 220 } : undefined}
        data-testid="xterm-container"
      />
    </div>
  );
}
