// Copy / paste plumbing for the xterm pane (#772).
//
// The daemon turns tmux mouse mode on for every node session so the wheel
// scrolls tmux's scrollback. Side effect: xterm.js honours the mouse-tracking
// request and reports every drag to the pty instead of building a browser
// selection. tmux then copies the text into *its* paste buffer on the daemon
// host and tries to hand it back with OSC 52, which xterm.js core drops. The
// user sees no highlight, Ctrl+Shift+C copies an empty string, and on a remote
// daemon the text is simply gone. Everything here is DOM-only and pure so it
// can be unit-tested without an xterm instance.

export function isMacPlatform(nav: Pick<Navigator, "platform" | "userAgent"> = navigator): boolean {
  return /Mac|iPhone|iPad|iPod/.test(nav.platform ?? "") || /Mac OS X/.test(nav.userAgent ?? "");
}

/** Events we re-dispatched ourselves; never rewrite them a second time. */
const forced = new WeakSet<Event>();

/**
 * xterm.js only lets the browser own a drag while the pty has mouse tracking
 * on when `shouldForceSelection` sees the platform modifier (Shift on
 * Linux/Windows, Option on macOS with `macOptionClickForcesSelection`). Build
 * the event that carries it, so a *plain* drag selects in the page and the
 * wheel keeps flowing to tmux untouched. Returns `null` when the event must
 * pass through as-is (mouse tracking off, already forced, not a select button).
 */
export function forcedSelectionEvent(
  e: MouseEvent,
  opts: { mouseTrackingActive: boolean; isMac: boolean },
): MouseEvent | null {
  if (!opts.mouseTrackingActive) return null;
  if (forced.has(e)) return null;
  // Primary button drags select; right button opens the context menu whose
  // paste path xterm handles itself. Middle click stays a pty event.
  if (e.button !== 0 && e.button !== 2) return null;
  if (opts.isMac ? e.altKey : e.shiftKey) return null;

  const init: MouseEventInit = {
    bubbles: e.bubbles,
    cancelable: e.cancelable,
    composed: e.composed,
    view: e.view,
    detail: e.detail,
    screenX: e.screenX,
    screenY: e.screenY,
    clientX: e.clientX,
    clientY: e.clientY,
    ctrlKey: e.ctrlKey,
    // Option on macOS is only a "force selection" key once
    // `macOptionClickForcesSelection` is set; otherwise it means column select.
    altKey: opts.isMac ? true : e.altKey,
    shiftKey: opts.isMac ? e.shiftKey : true,
    metaKey: e.metaKey,
    button: e.button,
    buttons: e.buttons,
    relatedTarget: e.relatedTarget,
  };
  const clone = new MouseEvent(e.type, init);
  forced.add(clone);
  return clone;
}

/**
 * #788: should a `mousemove` over the pane be stopped before xterm.js sees it?
 *
 * With all-motion tracking (DECSET 1003, what tmux mouse mode requests) xterm
 * reports every pointer move — button held or not — to the pty, and its
 * SelectionService clears the selection on *any* user input, mouse reports
 * included. So the first pixel of pointer travel after releasing a drag wipes
 * the selection #772 made possible, and Ctrl+C is back to SIGINT. Swallow
 * buttonless motion while a selection exists: tmux does nothing useful with a
 * hover, and drags (`buttons !== 0`), clicks and the wheel keep flowing.
 */
export function swallowsMotionReport(
  e: Pick<MouseEvent, "type" | "buttons">,
  opts: { hasSelection: boolean; mouseTrackingActive: boolean },
): boolean {
  if (e.type !== "mousemove") return false;
  if (!opts.mouseTrackingActive) return false;
  if (!opts.hasSelection) return false;
  return e.buttons === 0;
}

export type ClipboardKeyAction = "copy" | "paste" | "pass";

/**
 * What a keydown in the pane should do before xterm.js sees it.
 *
 * - Ctrl+C / Cmd+C **with a selection** copies it (return `false` to xterm so
 *   the browser fires its native `copy` event, which xterm serialises the
 *   selection into). Without a selection Ctrl+C stays SIGINT.
 * - Ctrl+V / Cmd+V pastes the same way Ctrl+Shift+V already does. xterm.js
 *   otherwise sends `^V` to the pty, which Claude Code reads as "paste image".
 * - Ctrl+Shift+C/V keep xterm's own handling; everything else passes.
 *
 * Both paths ride the native `copy`/`paste` events, which work over plain http:
 * no `navigator.clipboard`, hence no secure-context requirement.
 */
export function clipboardKeyAction(
  ev: Pick<KeyboardEvent, "type" | "key" | "ctrlKey" | "metaKey" | "shiftKey" | "altKey">,
  opts: { hasSelection: boolean; isMac: boolean },
): ClipboardKeyAction {
  if (ev.type !== "keydown") return "pass";
  const mod = opts.isMac ? ev.metaKey && !ev.ctrlKey : ev.ctrlKey && !ev.metaKey;
  if (!mod || ev.altKey || ev.shiftKey) return "pass";
  const key = ev.key.toLowerCase();
  if (key === "c") return opts.hasSelection ? "copy" : "pass";
  if (key === "v") return "paste";
  return "pass";
}

/**
 * Put `text` on the system clipboard from a user gesture. Prefers the async
 * Clipboard API (secure contexts only: https or localhost) and falls back to
 * `execCommand("copy")` on a throwaway textarea, which is what still works on
 * `http://<vps>:5172`. Resolves to whether anything reported success.
 */
export async function writeClipboardText(text: string, doc: Document = document): Promise<boolean> {
  const nav = doc.defaultView?.navigator;
  if (nav?.clipboard?.writeText) {
    try {
      await nav.clipboard.writeText(text);
      return true;
    } catch {
      // Fall through: denied permission or insecure context.
    }
  }
  const ta = doc.createElement("textarea");
  ta.value = text;
  ta.setAttribute("readonly", "");
  ta.style.position = "fixed";
  ta.style.top = "0";
  ta.style.left = "0";
  ta.style.opacity = "0";
  ta.style.pointerEvents = "none";
  doc.body.appendChild(ta);
  const active = doc.activeElement as HTMLElement | null;
  try {
    ta.focus();
    ta.select();
    return typeof doc.execCommand === "function" && doc.execCommand("copy");
  } catch {
    return false;
  } finally {
    doc.body.removeChild(ta);
    active?.focus?.();
  }
}
