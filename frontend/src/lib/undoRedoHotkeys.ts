// Pure keydown decision logic for canvas undo/redo (ADR-0014 / #226), split out
// of App's effect so it's unit-testable without rendering the whole app (the
// canvas + sockets don't render cleanly in jsdom). App keeps the wiring
// (addEventListener, the `hasEditTab` gate, the effect deps); this owns the
// branch logic and the input-focus guard.
//
// Bindings: Ctrl/Cmd+Z = undo, Ctrl/Cmd+Shift+Z or Ctrl/Cmd+Y = redo. While a
// text field (INPUT/TEXTAREA/SELECT/contenteditable) is focused the handler is
// inert, yielding to the browser's native field undo — unlike the Ctrl+S
// handler, which deliberately fires everywhere.
export function handleUndoRedoKeydown(
  e: KeyboardEvent,
  undo: () => void,
  redo: () => void,
): void {
  const el = (typeof document !== "undefined"
    ? (document.activeElement as HTMLElement | null)
    : null);
  const tag = el?.tagName;
  // `isContentEditable` is the canonical browser check (it also resolves
  // inheritance); the explicit `contentEditable === "true"` is a harmless
  // belt-and-suspenders that also satisfies jsdom, which doesn't implement
  // `isContentEditable`.
  if (
    tag === "INPUT" ||
    tag === "TEXTAREA" ||
    tag === "SELECT" ||
    el?.isContentEditable ||
    el?.contentEditable === "true"
  ) {
    return;
  }
  if (!(e.metaKey || e.ctrlKey)) return;
  const key = e.key.toLowerCase(); // Shift+Z reports "Z" on some layouts
  if (key === "z") {
    e.preventDefault();
    if (e.shiftKey) redo();
    else undo();
  } else if (key === "y") {
    e.preventDefault();
    redo();
  }
}
