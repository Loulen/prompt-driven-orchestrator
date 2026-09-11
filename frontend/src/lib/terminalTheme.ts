/**
 * The PTY terminal's palette (#759).
 *
 * xterm paints on a canvas and takes resolved colours, so it cannot read CSS
 * tokens. And ANSI colours are not app chrome: `red` must stay red whatever the
 * theme, only its lightness may move. They therefore live here, as two explicit
 * palettes, rather than in the semantic token layer of `index.css`.
 *
 * The dark palette is the one PDO has always shipped, unchanged. The light one
 * is new, and every ink colour clears AA on its ground (asserted, not assumed).
 */
import type { ResolvedTheme } from "./theme";

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

const DARK: TerminalTheme = {
  background: "#0f1115",
  foreground: "#e6e8eb",
  cursor: "#10b981",
  // #772: the old "#2a2d35" on a "#0f1115" background was one shade of grey
  // apart — a selection the user could not see. Accent at ~35% alpha keeps the
  // glyphs readable and the highlight obvious.
  selectionBackground: "#3b82f659",
  selectionInactiveBackground: "#3b82f633",
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
};

const LIGHT: TerminalTheme = {
  background: "#fbfbfc",
  foreground: "#14181f",
  cursor: "#03714f",
  selectionBackground: "#cfe4dc",
  selectionInactiveBackground: "#e3f0ea",
  // ANSI `black` is the ground, not ink: on a light terminal that is the page.
  black: "#fbfbfc",
  red: "#b91c1c",
  green: "#03714f",
  yellow: "#8f4506",
  blue: "#1d4ed8",
  magenta: "#6d28d9",
  cyan: "#0e6f82",
  white: "#3d4553",
  brightBlack: "#6b7280",
  brightRed: "#9f1239",
  brightGreen: "#065f46",
  brightYellow: "#7c3d05",
  brightBlue: "#1e40af",
  brightMagenta: "#5b21b6",
  brightCyan: "#0d5c6c",
  brightWhite: "#14181f",
};

export function terminalTheme(theme: ResolvedTheme): TerminalTheme {
  return theme === "light" ? LIGHT : DARK;
}
