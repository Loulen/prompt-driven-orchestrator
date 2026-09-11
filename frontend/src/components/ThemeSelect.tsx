/* Theme selector (#759): light · dark · system, device-local, written at the change.
   Lives in Settings › General › Interface next to Single-tab mode — same nature
   (a presentation preference this browser owns, never `instance_config`).

   `System` carries the theme it currently resolves to, because "System" alone
   tells you nothing about what you are about to get.

   Keyboard: the full ARIA radiogroup pattern — ONE tab stop on the group (roving
   tabindex), arrows to move-and-select, Home/End to jump. EffortPicker
   deliberately stopped at Tab + Enter/Space and said so; this control is the
   deliverable of an accessibility ticket, so a half-conformant radiogroup would
   undercut the very thing being shipped. */

import { useRef, type KeyboardEvent } from "react";
import { Monitor, Moon, Sun } from "lucide-react";
import { useTheme } from "../hooks/useTheme";
import type { ThemePreference } from "../lib/theme";

const OPTIONS: { id: ThemePreference; label: string; Icon: typeof Sun }[] = [
  { id: "light", label: "Light", Icon: Sun },
  { id: "dark", label: "Dark", Icon: Moon },
  { id: "system", label: "System", Icon: Monitor },
];

export default function ThemeSelect() {
  const { preference, resolved, setPreference } = useTheme();
  const buttons = useRef<(HTMLButtonElement | null)[]>([]);

  const move = (to: number) => {
    const index = (to + OPTIONS.length) % OPTIONS.length;
    setPreference(OPTIONS[index].id);
    buttons.current[index]?.focus();
  };

  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>, from: number) => {
    const step = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[event.key];
    if (step !== undefined) {
      event.preventDefault();
      move(from + step);
      return;
    }
    if (event.key === "Home" || event.key === "End") {
      event.preventDefault();
      move(event.key === "Home" ? 0 : OPTIONS.length - 1);
    }
  };

  return (
    <div role="radiogroup" aria-label="Theme" className="flex gap-1" data-testid="theme-select">
      {OPTIONS.map(({ id, label, Icon }, index) => {
        const selected = preference === id;
        return (
          <button
            key={id}
            type="button"
            role="radio"
            aria-checked={selected}
            // Roving tabindex: the group is a single tab stop, arrows navigate within it.
            tabIndex={selected ? 0 : -1}
            ref={(el) => {
              buttons.current[index] = el;
            }}
            data-testid={`theme-option-${id}`}
            onClick={() => setPreference(id)}
            onKeyDown={(event) => onKeyDown(event, index)}
            className={`flex flex-1 cursor-pointer items-center justify-center gap-1.5 rounded border px-2 py-1.5 font-medium transition-colors ${
              selected
                ? "border-acc bg-acc-bg text-acc"
                : "border-line-strong bg-bg-3 text-fg-4 hover:text-fg-3"
            }`}
            style={{ fontSize: "10px" }}
          >
            <Icon size={12} aria-hidden />
            <span>{label}</span>
            {id === "system" && (
              <span className="opacity-70" style={{ fontSize: "9px" }}>
                ({resolved})
              </span>
            )}
          </button>
        );
      })}
    </div>
  );
}
