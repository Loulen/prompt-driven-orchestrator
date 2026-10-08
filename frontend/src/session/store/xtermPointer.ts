import type { Terminal } from "@xterm/xterm";
import {
  clipboardKeyAction,
  forcedSelectionEvent,
  isMacPlatform,
  swallowsMotionReport,
} from "../model/terminalClipboard";

/**
 * The mouse, the clipboard keys and the wheel of a terminal pane, on its
 * `container`: selection owned by the browser even under tmux mouse mode,
 * Ctrl+C / Ctrl+V handed to the browser's copy and paste, the wheel scrolling
 * xterm's own buffer unless tmux tracks the mouse. Returns the cleanup.
 */
export function wirePointerAndKeys(term: Terminal, container: HTMLElement): () => void {
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

  return () => {
    container.removeEventListener("wheel", handleWheel, { capture: true });
    container.removeEventListener("mousedown", handleMouseDown, { capture: true });
    container.removeEventListener("mousemove", handleMouseMove, { capture: true });
  };
}
