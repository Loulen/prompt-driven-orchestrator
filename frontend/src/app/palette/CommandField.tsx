import { useEffect } from "react";
import { Search } from "lucide-react";
import { Kbd } from "../../shared/ui/kit";
import { isMacPlatform } from "../../session/model/terminalClipboard";
import { usePalette } from "./paletteStore";

/**
 * The top bar's command field: a field-shaped button in the middle of the bar
 * that opens the command palette, as Ctrl+K (Cmd+K) does from anywhere.
 */
export default function CommandField() {
  const open = usePalette((s) => s.open);
  const show = usePalette((s) => s.show);
  usePaletteHotkey();
  const mod = isMacPlatform() ? "⌘" : "Ctrl";
  return (
    <button
      type="button"
      className="sh-cmd"
      onClick={show}
      aria-label="Search"
      aria-expanded={open}
      aria-haspopup="dialog"
      title={`Search (${mod}+K)`}
      data-testid="command-field"
    >
      <Search size={13} strokeWidth={1.5} aria-hidden />
      <span className="sh-cmd__text">Search</span>
      <Kbd>{`${mod} K`}</Kbd>
    </button>
  );
}

/** Ctrl+K (Cmd+K) opens the palette, or closes it when open. */
function usePaletteHotkey() {
  const toggle = usePalette((s) => s.toggle);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.altKey || e.shiftKey || e.key.toLowerCase() !== "k") return;
      e.preventDefault();
      toggle();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [toggle]);
}
