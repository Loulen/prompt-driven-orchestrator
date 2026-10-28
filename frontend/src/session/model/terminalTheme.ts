/**
 * The PTY terminal's palette (#759).
 *
 * xterm paints on a canvas and takes resolved colours, so it cannot read CSS
 * tokens. And ANSI colours are not app chrome: `red` must stay red. They
 * therefore live here, as one explicit palette, rather than in the tokens.
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
  background: "#0b0b0d",
  foreground: "#aeb1b5",
  cursor: "#d2d4d7",
  // #772: a selection the user can see — blue at ~35% alpha keeps the glyphs
  // readable and the highlight obvious.
  selectionBackground: "#3b82f659",
  selectionInactiveBackground: "#3b82f633",
  black: "#0b0b0d",
  red: "#ef4444",
  green: "#10b981",
  yellow: "#f59e0b",
  blue: "#3b82f6",
  magenta: "#8b5cf6",
  cyan: "#06b6d4",
  white: "#d2d4d7",
  brightBlack: "#5a6270",
  brightRed: "#f87171",
  brightGreen: "#34d399",
  brightYellow: "#fbbf24",
  brightBlue: "#60a5fa",
  brightMagenta: "#a78bfa",
  brightCyan: "#22d3ee",
  brightWhite: "#f8fafc",
};

/** The terminal's palette: always the dark screen. */
export function terminalTheme(): TerminalTheme {
  return DARK;
}
