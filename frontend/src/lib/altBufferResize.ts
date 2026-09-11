// #771 — xterm.js 6.0.0 corrupts the alternate screen on the first resize that
// follows a scroll inside a partial scroll region.
//
// tmux keeps its status bar out of the scroll region (`ESC[1;<rows-1>r`), so
// every line feed the hosted TUI emits at the bottom of the pane scrolls a
// region whose top is row 0 and whose bottom is *not* the last row. xterm's
// `BufferService.scroll` takes its "whole screen" fast path whenever the region
// top is 0: it pushes a line and bumps `ybase` — a scrollback, on the alternate
// buffer, which by design has none. The pane keeps rendering fine at a fixed
// size, but the alternate buffer's `resize` does not account for that `ybase`,
// so the first grow leaves the viewport reaching past the line store and the
// redraw tmux sends smears one line over every new row (the "43 identical
// rows" of the report). The next resize happens to land on a consistent state,
// hence "click twice".
//
// The poisoned state is observable from the public API: an alternate buffer
// whose `baseY` is not 0. Leaving and re-entering the alternate screen
// (`DECRST 1049` then `DECSET 1049`) makes xterm rebuild it from scratch
// (`ybase` back to 0, one line per row) without touching any other mode
// (cursor keys, mouse tracking, bracketed paste all survive — see the test).
// The screen is blank for the few milliseconds until tmux's own redraw for the
// new size lands, which it always does since the size did change.

export const ALT_BUFFER_RESET = "\x1b[?1049l\x1b[?1049h";

/** The slice of xterm's `Terminal` this module needs; keeps the tests off jsdom. */
export interface ResizableTerminal {
  readonly cols: number;
  readonly rows: number;
  readonly buffer: { readonly active: { readonly type: "normal" | "alternate"; readonly baseY: number } };
  write(data: string, callback?: () => void): void;
}

export interface Dimensions {
  cols: number;
  rows: number;
}

/** True when xterm's alternate buffer is in the state its own resize cannot handle. */
export function hasPoisonedAltBuffer(term: ResizableTerminal): boolean {
  const active = term.buffer?.active;
  return active !== undefined && active.type === "alternate" && active.baseY > 0;
}

/**
 * Apply a size change, resetting the alternate buffer first when the change
 * would otherwise corrupt it. `apply` performs the actual resize (fit + PTY
 * resize message); it runs synchronously when no reset is needed, or once
 * xterm has processed the reset sequence otherwise.
 */
export function resizeAvoidingAltBufferCorruption(
  term: ResizableTerminal,
  proposed: Dimensions | undefined,
  apply: () => void,
): void {
  const changes =
    proposed !== undefined && (proposed.cols !== term.cols || proposed.rows !== term.rows);
  if (changes && hasPoisonedAltBuffer(term)) {
    term.write(ALT_BUFFER_RESET, apply);
    return;
  }
  apply();
}
