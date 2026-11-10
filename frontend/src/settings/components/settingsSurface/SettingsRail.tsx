import { Activity, Bell, Bot, Briefcase, Container, Palette, Puzzle, RadioTower, SlidersHorizontal, type LucideIcon } from "lucide-react";
import type { ReactNode } from "react";

/** The icon of each core category on the rail. */
const CATEGORY_ICONS: Record<string, LucideIcon> = {
  general: SlidersHorizontal,
  appearance: Palette,
  notifications: Bell,
  agents: Bot,
  sandbox: Container,
  projects: Briefcase,
  telemetry: RadioTower,
  diagnostics: Activity,
  extensions: Puzzle,
};

export interface SettingsRailEntry {
  id: string;
  label: string;
  /** An extension page's icon; a core category's comes from its id. */
  icon?: ReactNode;
  dirty?: boolean;
  /** Starts the extensions' group, under a rule. */
  group?: boolean;
}

/**
 * The category rail of the Settings: the core's categories, then — under a rule — the
 * Extensions entry and each extension page. One tab stop; ↑↓ move and select.
 */
export default function SettingsRail({
  entries,
  active,
  onSelect,
}: {
  entries: SettingsRailEntry[];
  /** None under a search, which spans every category. */
  active: string | null;
  onSelect: (id: string) => void;
}) {
  const groupAt = entries.findIndex((entry) => entry.group);
  // The one tab stop: the current category, else the first.
  const stop = active ?? entries[0]?.id;
  const item = (entry: SettingsRailEntry) => {
    const Icon = CATEGORY_ICONS[entry.id];
    const selected = entry.id === active;
    return (
      <button
        key={entry.id}
        type="button"
        role="tab"
        aria-selected={selected}
        tabIndex={entry.id === stop ? 0 : -1}
        data-testid={`settings-category-${entry.id}`}
        data-dirty={entry.dirty ? "true" : undefined}
        onClick={() => onSelect(entry.id)}
        className="set-nav__item"
        title={entry.label}
      >
        {entry.icon ?? (Icon && <Icon size={14} strokeWidth={1.5} aria-hidden />)}
        <span className="set-nav__label">{entry.label}</span>
        {entry.dirty && (
          <span
            role="img"
            aria-label="Unsaved changes"
            title="Unsaved changes"
            data-testid={`settings-category-${entry.id}-dirty`}
            className="set-dirty"
          />
        )}
      </button>
    );
  };
  return (
    <nav
      className="set-nav"
      role="tablist"
      aria-orientation="vertical"
      aria-label="Settings categories"
      onKeyDown={(event) => {
        if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
        event.preventDefault();
        const index = entries.findIndex((entry) => entry.id === stop);
        const delta = event.key === "ArrowDown" ? 1 : -1;
        const next = entries[(index + delta + entries.length) % entries.length];
        onSelect(next.id);
        event.currentTarget
          .querySelector<HTMLElement>(`[data-testid='settings-category-${next.id}']`)
          ?.focus();
      }}
    >
      {(groupAt < 0 ? entries : entries.slice(0, groupAt)).map(item)}
      {groupAt >= 0 && <div className="set-nav__group">{entries.slice(groupAt).map(item)}</div>}
    </nav>
  );
}
