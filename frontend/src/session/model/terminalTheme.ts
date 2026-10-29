/**
 * The PTY terminal's palette (#759).
 *
 * xterm paints on a canvas and takes resolved colours, so the palette is read
 * from its tokens (`session/terminal.css`) at the time the terminal opens. ANSI
 * colours are not app chrome: `red` must stay red, so they are tokens of their
 * own rather than roles of the theme.
 */

import { cssColor } from "../../lib/cssColor";

export interface TerminalTheme {
  background: string;
  foreground: string;
  cursor: string;
  selectionBackground: string;
  selectionInactiveBackground: string;
  black: string;
  red: string;
  green: string;
  yellow: string;
  blue: string;
  magenta: string;
  cyan: string;
  white: string;
  brightBlack: string;
  brightRed: string;
  brightGreen: string;
  brightYellow: string;
  brightBlue: string;
  brightMagenta: string;
  brightCyan: string;
  brightWhite: string;
}

/**
 * The slots that carry characters. `black` is excluded on purpose: in the ANSI
 * model it IS the ground, so measuring it against the ground is meaningless.
 */
export const TERMINAL_INK = [
  "foreground",
  "red",
  "green",
  "yellow",
  "blue",
  "magenta",
  "cyan",
  "white",
  "brightBlack",
  "brightRed",
  "brightGreen",
  "brightYellow",
  "brightBlue",
  "brightMagenta",
  "brightCyan",
  "brightWhite",
] as const satisfies readonly (keyof TerminalTheme)[];

/**
 * The terminal is an always-dark screen, whatever the theme of the interface
 * (a lit display): its ground is the paper of the dark theme darkened
 * (`color-mix(in oklab, paper 82%, black)`, `.ri-term-screen`), its text the
 * secondary ink, its cursor the ink. The values are the `--term-*` tokens of
 * `session/terminal.css`, resolved, since xterm takes no `var()`.
 */
export function terminalTheme(): TerminalTheme {
  const slot = (name: keyof TerminalTheme) =>
    cssColor(`--term-${name.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`)}`);
  return {
    background: slot("background"),
    foreground: slot("foreground"),
    cursor: slot("cursor"),
    // #772: a selection the user can see — blue at ~35% alpha keeps the glyphs
    // readable and the highlight obvious.
    selectionBackground: slot("selectionBackground"),
    selectionInactiveBackground: slot("selectionInactiveBackground"),
    black: slot("black"),
    red: slot("red"),
    green: slot("green"),
    yellow: slot("yellow"),
    blue: slot("blue"),
    magenta: slot("magenta"),
    cyan: slot("cyan"),
    white: slot("white"),
    brightBlack: slot("brightBlack"),
    brightRed: slot("brightRed"),
    brightGreen: slot("brightGreen"),
    brightYellow: slot("brightYellow"),
    brightBlue: slot("brightBlue"),
    brightMagenta: slot("brightMagenta"),
    brightCyan: slot("brightCyan"),
    brightWhite: slot("brightWhite"),
  };
}
