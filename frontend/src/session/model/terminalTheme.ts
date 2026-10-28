/**
 * The PTY terminal's palette (#759).
 *
 * xterm paints on a canvas and takes resolved colours, so it cannot read CSS
 * tokens. And ANSI colours are not app chrome: `red` must stay red. They
 * therefore live here, as one explicit palette of `rgb()` values, rather than
 * in the tokens.
 */

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
 * secondary ink, its cursor the ink.
 */
const DARK: TerminalTheme = {
  background: "rgb(11, 11, 13)",
  foreground: "rgb(174, 177, 181)",
  cursor: "rgb(210, 212, 215)",
  // #772: a selection the user can see — blue at ~35% alpha keeps the glyphs
  // readable and the highlight obvious.
  selectionBackground: "rgba(59, 130, 246, 0.35)",
  selectionInactiveBackground: "rgba(59, 130, 246, 0.2)",
  black: "rgb(11, 11, 13)",
  red: "rgb(239, 68, 68)",
  green: "rgb(16, 185, 129)",
  yellow: "rgb(245, 158, 11)",
  blue: "rgb(59, 130, 246)",
  magenta: "rgb(139, 92, 246)",
  cyan: "rgb(6, 182, 212)",
  white: "rgb(210, 212, 215)",
  brightBlack: "rgb(90, 98, 112)",
  brightRed: "rgb(248, 113, 113)",
  brightGreen: "rgb(52, 211, 153)",
  brightYellow: "rgb(251, 191, 36)",
  brightBlue: "rgb(96, 165, 250)",
  brightMagenta: "rgb(167, 139, 250)",
  brightCyan: "rgb(34, 211, 238)",
  brightWhite: "rgb(248, 250, 252)",
};

/** The terminal's palette: always the dark screen. */
export function terminalTheme(): TerminalTheme {
  return DARK;
}
