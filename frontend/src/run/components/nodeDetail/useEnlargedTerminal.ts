import { useCallback, useEffect, useRef, useState, type PointerEvent } from "react";
import { useTerminalOverlayStore } from "../../../session/store/terminalOverlayStore";
import type { GesturePopover } from "./EnlargedTerminalBar";

/**
 * The behaviour of the « Terminal agrandi » (#44, core #968): the overlay's
 * exits, the keyboard it keeps from the app, the focus it gives the agent, and
 * the verdict popover of its gestures. `collapse` (a stable callback) brings
 * the terminal back to the split view. While `keyboardSuspended` (a modal over
 * the overlay, the import), the keyboard is the modal's.
 */
export function useEnlargedTerminal(
  expanded: boolean,
  collapse: () => void,
  keyboardSuspended = false,
) {
  const frameRef = useRef<HTMLDivElement>(null);
  const popoverRef = useRef<HTMLDivElement>(null);
  // Closing on the backdrop needs the press AND the release there: a selection
  // started in the terminal and released over the backdrop must not close it.
  const backdropPressed = useRef(false);
  const [gesturePopover, setGesturePopover] = useState<GesturePopover>(null);
  if (!expanded && gesturePopover !== null) setGesturePopover(null);

  // The app's panel groups stop resizing while the overlay covers them.
  useEffect(() => {
    if (!expanded) return;
    const { opened, closed } = useTerminalOverlayStore.getState();
    opened();
    return closed;
  }, [expanded]);

  // On open, the keyboard goes to the agent straight away.
  useEffect(() => {
    if (!expanded) return;
    frameRef.current
      ?.querySelector<HTMLTextAreaElement>(".xterm-helper-textarea")
      ?.focus();
  }, [expanded]);

  // While enlarged, the app behind it gets no keystroke: Escape closes the
  // overlay, every other key stops here (canvas shortcuts, the tour's Escape).
  // Capture phase on `window`, so we run before anyone else; but a keystroke
  // aimed at the terminal's textarea is the agent's (Escape interrupts Claude
  // Code), and passes untouched, the rule of `RunShellModal`.
  useEffect(() => {
    if (!expanded || keyboardSuspended) return;
    const onKey = (e: KeyboardEvent) => {
      const active = document.activeElement;
      if (active && active.classList.contains("xterm-helper-textarea")) return;
      e.stopImmediatePropagation();
      if (e.key === "Escape") {
        e.preventDefault();
        collapse();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [expanded, collapse, keyboardSuspended]);

  // The gesture popover closes on any press outside it.
  useEffect(() => {
    if (gesturePopover === null) return;
    const onDown = (e: globalThis.PointerEvent) => {
      if (e.target instanceof Node && popoverRef.current?.contains(e.target)) return;
      setGesturePopover(null);
    };
    document.addEventListener("pointerdown", onDown, true);
    return () => document.removeEventListener("pointerdown", onDown, true);
  }, [gesturePopover]);

  // A press inside the frame forgets an earlier press on the backdrop.
  const onFramePointerDownCapture = useCallback(() => {
    backdropPressed.current = false;
  }, []);
  const onBackdropPointerDown = useCallback((e: PointerEvent<HTMLDivElement>) => {
    backdropPressed.current = e.target === e.currentTarget;
  }, []);
  const onBackdropPointerUp = useCallback(
    (e: PointerEvent<HTMLDivElement>) => {
      const pressed = backdropPressed.current;
      backdropPressed.current = false;
      if (pressed && e.target === e.currentTarget) collapse();
    },
    [collapse],
  );

  return {
    frameRef,
    popoverRef,
    gesturePopover,
    setGesturePopover,
    onFramePointerDownCapture,
    onBackdropPointerDown,
    onBackdropPointerUp,
  };
}
