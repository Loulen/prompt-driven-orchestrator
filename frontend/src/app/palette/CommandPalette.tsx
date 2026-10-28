import { useEffect, useRef } from "react";
import { Search } from "lucide-react";
import { usePalette } from "./paletteStore";

/**
 * The command palette under the top bar: a query field and the results. It
 * lists nothing yet: the extensions' Actions and search Sources fill it.
 * Escape or a click outside closes it, and the focus goes back where it was:
 * the command field when it opened the palette.
 */
export default function CommandPalette() {
  const open = usePalette((s) => s.open);
  const query = usePalette((s) => s.query);
  const setQuery = usePalette((s) => s.setQuery);
  const hide = usePalette((s) => s.hide);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (!open) return;
    const before = document.activeElement;
    input.current?.focus();
    return () => {
      if (before instanceof HTMLElement && before !== document.body && before.isConnected) before.focus();
    };
  }, [open]);
  if (!open) return null;
  return (
    <div className="sh-pal-scrim" onMouseDown={hide} data-testid="command-palette-scrim">
      <div
        className="sh-pal"
        role="dialog"
        aria-label="Command palette"
        data-testid="command-palette"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            e.preventDefault();
            e.stopPropagation();
            hide();
          }
        }}
      >
        <div className="sh-pal__field">
          <Search size={13} strokeWidth={1.5} aria-hidden />
          <input
            ref={input}
            className="sh-pal__input"
            placeholder="Search"
            aria-label="Search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            data-testid="command-palette-input"
          />
        </div>
        <div className="sh-pal__list" role="listbox" aria-label="Results">
          <div className="sh-pal__empty" data-testid="command-palette-empty">
            No results
          </div>
        </div>
      </div>
    </div>
  );
}
