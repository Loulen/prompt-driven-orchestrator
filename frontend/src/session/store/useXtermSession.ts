import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import "@xterm/xterm/css/xterm.css";
import {
  clipboardKeyAction,
  forcedSelectionEvent,
  isMacPlatform,
  swallowsMotionReport,
  writeClipboardText,
} from "../model/terminalClipboard";
import { resizeAvoidingAltBufferCorruption } from "../model/altBufferResize";
import {
  isTakenOver,
  parseRoleFrame,
  SOLO,
  TAKE_CONTROL_FRAME,
  TAKEN_OVER_NOTICE_MS,
  type TerminalRole,
} from "../model/terminalRole";
import { posteId } from "../model/poste";
import { BASE_FONT_SIZE, spectatorFontSize } from "../model/spectatorFit";
import { terminalTheme } from "../model/terminalTheme";
import {
  sendResize,
  TERMINAL_INSET_PX,
  toTerminalNewlines,
  type FrozenPane,
  type PaneMode,
} from "../model/terminalPane";
import { useTheme } from "../../hooks/useTheme";
import { ABNORMAL_CLOSURE, probeSignIn } from "../../shared/http/signIn";

/** What the xterm of one session exposes to its toolbar. */
export interface XtermSession {
  connected: boolean;
  /** #867: the role the daemon gave this socket. */
  termRole: TerminalRole;
  /** #870: « Another browser took control », up for a few seconds after this
   *  browser lost the hand. */
  takenOver: boolean;
  /** #772: xterm's selection lives outside React. */
  hasSelection: boolean;
  copyFeedback: "copied" | "failed" | null;
  copySelection: () => void;
  takeControl: () => void;
}

/**
 * The xterm of a session and its PTY socket (`WS /sessions/<session>/pty`): the
 * live attach, or — for a frozen pane — the snapshot written straight into the
 * same xterm with no socket at all (#617).
 */
export function useXtermSession(
  containerRef: RefObject<HTMLDivElement | null>,
  session: string,
  mode: PaneMode,
  frozen: FrozenPane | null,
): XtermSession {
  const terminalRef = useRef<Terminal | null>(null);
  // #759: the live theme. Used to build the palette at mount and to repaint an
  // already-open terminal when the theme switches (xterm paints on a canvas, so
  // it cannot follow a CSS token on its own).
  const { resolved } = useTheme();
  const fitAddonRef = useRef<FitAddon | null>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const [connected, setConnected] = useState(false);
  // #867: the ref is what the xterm callbacks read (they outlive renders); the
  // state drives the toolbar.
  const [termRole, setTermRole] = useState<TerminalRole>(SOLO);
  const roleRef = useRef<TerminalRole>(SOLO);
  const [takenOver, setTakenOver] = useState(false);
  const takenOverTimer = useRef<number | null>(null);
  const [hasSelection, setHasSelection] = useState(false);
  const [copyFeedback, setCopyFeedback] = useState<"copied" | "failed" | null>(null);

  // #772: an explicit Copy button for the keyboard-less case (touch, remote
  // desktop) and as the visible hint that selection is browser-side. Uses the
  // execCommand fallback, so it works on a plain-http remote origin too.
  const copySelection = useCallback(async () => {
    const term = terminalRef.current;
    if (!term || !term.hasSelection()) return;
    const ok = await writeClipboardText(term.getSelection());
    setCopyFeedback(ok ? "copied" : "failed");
    if (ok) term.clearSelection();
    window.setTimeout(() => setCopyFeedback(null), 1500);
  }, []);

  // #870: a spectator takes control. No confirmation, and no optimistic change:
  // the daemon answers with the new role, which is what flips the toolbar.
  const takeControl = useCallback(() => {
    const ws = wsRef.current;
    if (ws && ws.readyState === WebSocket.OPEN) ws.send(TAKE_CONTROL_FRAME);
  }, []);

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
      fontSize: BASE_FONT_SIZE,
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
    // #911: the grid started flush against the panel's edge, and the first
    // column read clipped (`mplementer:`). The inset goes on xterm's own element,
    // whose padding FitAddon subtracts — padding the container instead would be
    // counted as grid width and push the last column out.
    if (term.element) term.element.style.paddingLeft = `${TERMINAL_INSET_PX}px`;
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
    const wsUrl = `${protocol}//${window.location.host}/sessions/${encodeURIComponent(session)}/pty?poste=${encodeURIComponent(posteId())}`;
    const ws = isFrozen ? null : new WebSocket(wsUrl);
    if (ws) {
      ws.binaryType = "arraybuffer";
    }
    wsRef.current = ws;

    if (isFrozen) {
      term.write(toTerminalNewlines(frozen?.content ?? ""));
    }

    // #867: size the grid for the current role. Pilot and solo fit their own
    // area at the normal font, as always. A spectator takes the pilot's exact
    // grid — and tells its own PTY so, which is what removes tmux's cropping and
    // dots — with the font shrunk to fit, down to the floor; below it the area
    // scrolls.
    //
    // #771: a live pane in the alternate screen may be in the state xterm's
    // resize corrupts (see `altBufferResize.ts`); the helper resets it first and
    // runs the resize once xterm has processed the reset. A frozen pane has no
    // alternate screen to reset, and nothing to redraw it — plain fit.
    const layout = () => {
      const current = roleRef.current;
      if (current.role === "spectator") {
        const pilot = current.pilot;
        const size = spectatorFontSize(pilot, (fontSize) => {
          term.options.fontSize = fontSize;
          return fitAddon.proposeDimensions();
        });
        term.options.fontSize = size;
        resizeAvoidingAltBufferCorruption(term, pilot, () => {
          term.resize(pilot.cols, pilot.rows);
          if (ws) sendResize(ws, pilot);
        });
        return;
      }
      if (term.options && term.options.fontSize !== BASE_FONT_SIZE) {
        term.options.fontSize = BASE_FONT_SIZE;
      }
      const apply = () => {
        fitAddon.fit();
        if (ws) sendResize(ws, fitAddon.proposeDimensions());
      };
      if (!ws) {
        apply();
        return;
      }
      resizeAvoidingAltBufferCorruption(term, fitAddon.proposeDimensions(), apply);
    };

    const clearTakenOver = () => {
      if (takenOverTimer.current !== null) {
        window.clearTimeout(takenOverTimer.current);
        takenOverTimer.current = null;
      }
      setTakenOver(false);
    };

    const applyRole = (next: TerminalRole) => {
      if (isTakenOver(roleRef.current, next)) {
        clearTakenOver();
        setTakenOver(true);
        takenOverTimer.current = window.setTimeout(() => {
          takenOverTimer.current = null;
          setTakenOver(false);
        }, TAKEN_OVER_NOTICE_MS);
      } else if (next.role !== "spectator") {
        clearTakenOver();
      }
      roleRef.current = next;
      setTermRole(next);
      layout();
    };

    ws?.addEventListener("open", () => {
      setConnected(true);
      sendResize(ws, fitAddon.proposeDimensions());
    });

    ws?.addEventListener("message", (event) => {
      if (event.data instanceof ArrayBuffer) {
        term.write(new Uint8Array(event.data));
      } else if (typeof event.data === "string") {
        // Text frames carry the daemon's control messages (the role); anything
        // else is written as before.
        const role = parseRoleFrame(event.data);
        if (role) applyRole(role);
        else term.write(event.data);
      }
    });

    // A closed socket has no role left: back to the plain fit.
    const dropRole = () => {
      setConnected(false);
      if (roleRef.current.role !== "solo") applyRole(SOLO);
    };
    ws?.addEventListener("close", dropRole);
    ws?.addEventListener("error", dropRole);
    // 0.8: a refused upgrade (1006, no handshake) says nothing to the browser;
    // ask whether the UI must sign in first.
    ws?.addEventListener("close", (event) => {
      if (event.code === ABNORMAL_CLOSURE) void probeSignIn();
    });

    // ADR-0075 §3: a spectator is read-only. The daemon drops its input anyway;
    // not sending it keeps the pane's own echo from lying about what happened.
    const canType = () => roleRef.current.role !== "spectator";

    const inputDisposable = term.onData((data) => {
      if (ws && ws.readyState === WebSocket.OPEN && canType()) {
        const encoder = new TextEncoder();
        ws.send(encoder.encode(data));
      }
    });

    const binaryDisposable = term.onBinary((data) => {
      if (ws && ws.readyState === WebSocket.OPEN && canType()) {
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

    const resizeObserver = new ResizeObserver(layout);
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
      roleRef.current = SOLO;
      setTermRole(SOLO);
      clearTakenOver();
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

  return {
    connected,
    termRole,
    takenOver,
    hasSelection,
    copyFeedback,
    copySelection: () => void copySelection(),
    takeControl,
  };
}
