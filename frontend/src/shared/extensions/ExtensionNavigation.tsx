import { useContributions, useLocationContext } from "./catalogue";
import { openPage } from "./deepLink";

/**
 * The `topbar/start` location (the 0.1 navigation slot): one text entry per
 * page in the top bar, each opening its page on the full-window surface
 * Settings and Stats share, the location's pages on its rail
 * (`ExtensionPageHost`). Nothing without a contribution.
 */
export default function ExtensionNavigation() {
  const context = useLocationContext("topbar/start");
  const entries = useContributions("topbar/start", context);
  if (entries.length === 0) return null;
  return (
    <nav className="flex items-center gap-1" aria-label="Extensions" data-testid="extension-nav">
      {entries.map((entry) => (
        <button
          key={entry.key}
          type="button"
          onClick={() =>
            openPage({ key: entry.key, extension: entry.extension.id, location: "topbar/start", path: "" })
          }
          data-testid={`extension-nav-${entry.key}`}
          className="rounded px-2 py-0.5 text-fg-3 transition-colors hover:bg-bg-5 hover:text-fg"
        >
          {entry.contribution.label}
        </button>
      ))}
    </nav>
  );
}
