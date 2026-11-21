/**
 * CORE-284: Shift+Enter inserts a new line in a web terminal. xterm.js sends
 * the same `\r` for Enter and Shift+Enter, so Claude Code cannot tell them
 * apart; Alt+Enter's sequence (ESC then CR) is the new line it understands,
 * and it crosses the WebSocket and tmux unchanged.
 */
export const ALT_ENTER = "\x1b\r";

/**
 * What the custom key handler does with a key event: `newline` injects
 * {@link ALT_ENTER} (keydown only), `swallow` drops the event (the keypress
 * and keyup of the same Shift+Enter), `pass` leaves it to xterm.
 */
export type NewlineKeyAction = "newline" | "swallow" | "pass";

export function newlineKeyAction(
  ev: Pick<
    KeyboardEvent,
    "type" | "key" | "shiftKey" | "ctrlKey" | "altKey" | "metaKey" | "isComposing"
  >,
): NewlineKeyAction {
  const shiftEnter =
    ev.key === "Enter" &&
    ev.shiftKey &&
    !ev.ctrlKey &&
    !ev.altKey &&
    !ev.metaKey &&
    !ev.isComposing;
  if (!shiftEnter) return "pass";
  return ev.type === "keydown" ? "newline" : "swallow";
}
