import { useState } from "react";
import FullWindowShell from "../ui/FullWindowShell";
import ExtensionBoundary from "./ExtensionBoundary";
import { useSlot, type SlotElement, type SlotProps } from "./slots";

/**
 * The navigation slot: one entry per element in the top bar, each opening its
 * page on the full-window surface Settings and Stats share, the entries on its
 * rail. Nothing without a contribution.
 */
export default function ExtensionNavigation() {
  const entries = useSlot("navigation");
  const [open, setOpen] = useState<string | null>(null);
  if (entries.length === 0) return null;
  const active = entries.find((e) => e.key === open) ?? null;
  return (
    <>
      <nav className="flex items-center gap-1" aria-label="Extensions" data-testid="extension-nav">
        {entries.map((entry) => (
          <button
            key={entry.key}
            type="button"
            onClick={() => setOpen(entry.key)}
            data-testid={`extension-nav-${entry.key}`}
            className="rounded px-2 py-0.5 text-fg-3 transition-colors hover:bg-bg-5 hover:text-fg"
          >
            {entry.slot.label}
          </button>
        ))}
      </nav>
      {active && (
        <ExtensionPage
          entries={entries}
          active={active}
          onSelect={setOpen}
          onClose={() => setOpen(null)}
        />
      )}
    </>
  );
}

function ExtensionPage({
  entries,
  active,
  onSelect,
  onClose,
}: {
  entries: SlotElement<SlotProps>[];
  active: SlotElement<SlotProps>;
  onSelect: (key: string) => void;
  onClose: () => void;
}) {
  const { Component, extension } = active;
  return (
    <FullWindowShell
      title={active.slot.label}
      testId="extension-page"
      rail={entries.map((e) => ({ id: e.key, label: e.slot.label }))}
      activeRail={active.key}
      onRailChange={onSelect}
      railWidth={176}
      railAriaLabel="Extension pages"
      railTestIdPrefix="extension-page"
      onClose={onClose}
      closeLabel={`Close ${active.slot.label}`}
      mainClassName="min-h-0 min-w-0 flex-1 overflow-y-auto px-7 py-5"
    >
      <ExtensionBoundary key={active.key} extensionId={extension.id}>
        <Component extension={{ id: extension.id, name: extension.name }} />
      </ExtensionBoundary>
    </FullWindowShell>
  );
}
