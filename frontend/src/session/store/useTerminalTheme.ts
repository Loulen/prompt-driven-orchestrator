import { useEffect, type RefObject } from "react";
import type { Terminal } from "@xterm/xterm";
import { useTheme } from "../../hooks/useTheme";
import { terminalTheme } from "../model/terminalTheme";

function repaint(term: Terminal | null): void {
  if (term?.options) term.options.theme = terminalTheme();
}

/**
 * ADR-0093: xterm takes resolved colours, so a switch of Theme reads the
 * terminal's palette again. Not a switch of mode: the screen is always dark,
 * its `--term-*` tokens do not depend on the mode.
 */
export function useTerminalTheme(terminal: RefObject<Terminal | null>): void {
  const { theme } = useTheme();
  useEffect(() => repaint(terminal.current), [terminal, theme]);
}
