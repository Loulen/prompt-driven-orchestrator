import { useEffect, type ReactNode } from "react";
import { X } from "lucide-react";

export interface RailItem {
  id: string;
  label: string;
  /** Unsaved edits live under this entry — renders an amber dot (#690). */
  dirty?: boolean;
}

interface Props {
  title: string;
  /** Rendered after the title, in the header row (Stats' period presets, etc.). */
  headerExtras?: ReactNode;
  /** Right-aligned header content, before the close button (hints, refresh…). */
  headerActions?: ReactNode;
  rail: RailItem[];
  activeRail: string;
  onRailChange: (id: string) => void;
  /** Rail width in px. Stats keeps 144; Settings needs 176 for "Sandbox & worktrees". */
  railWidth?: number;
  railAriaLabel: string;
  /** `data-testid` prefix of a rail entry: `${prefix}-${id}`. */
  railTestIdPrefix: string;
  /** Right-side drawer (Stats' pricing details, Settings' skill bank). */
  drawer?: ReactNode;
  /** Spans the pane, not the rail. Settings' Save footer. */
  footer?: ReactNode;
  /**
   * Escape handler. Called instead of `onClose` so the host can close a drawer or ask a
   * confirmation first. The shell already ignores Escape while a tooltip is open.
   */
  onEscape?: () => void;
  onClose: () => void;
  closeLabel: string;
  testId: string;
  /** Class of the `<main>` slot. Defaults to a flex row that fills the pane. */
  mainClassName?: string;
  children: ReactNode;
}

/**
 * The full-window surface Stats and Settings share (#690): overlay, header (title + ✕),
 * left rail navigated with ↑↓, main slot, optional right drawer and optional footer.
 *
 * The shell owns Escape: ignored while a tooltip is open (the tooltip consumes it), else
 * delegated to `onEscape` (drawer first, then confirmation, then close — the host decides).
 */
export default function FullWindowShell({
  title,
  headerExtras,
  headerActions,
  rail,
  activeRail,
  onRailChange,
  railWidth = 144,
  railAriaLabel,
  railTestIdPrefix,
  drawer,
  footer,
  onEscape,
  onClose,
  closeLabel,
  testId,
  mainClassName = "flex min-h-0 min-w-0 flex-1",
  children,
}: Props) {
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      if (
        document.querySelector(
          '[data-testid="tooltip-content"][data-state="delayed-open"], [data-testid="tooltip-content"][data-state="instant-open"]',
        )
      ) {
        return;
      }
      (onEscape ?? onClose)();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onEscape, onClose]);

  return (
    <div className="fixed inset-0 z-50 bg-bg-2">
      <div className="relative flex h-screen w-screen flex-col bg-bg-4" data-testid={testId}>
        <header className="flex min-h-14 items-center gap-4 border-b border-line px-4">
          <h2 className="font-semibold text-fg">{title}</h2>
          {headerExtras}
          <div className="ml-auto flex items-center gap-2">{headerActions}</div>
          <button
            type="button"
            onClick={onClose}
            aria-label={closeLabel}
            className="grid h-7 w-7 place-items-center rounded text-fg-3 hover:bg-bg-5"
          >
            <X size={15} />
          </button>
        </header>

        <div className="flex min-h-0 flex-1">
          <nav
            className="flex shrink-0 flex-col gap-1 border-r border-line bg-bg-3 p-3"
            style={{ width: railWidth }}
            role="tablist"
            aria-label={railAriaLabel}
            onKeyDown={(event) => {
              if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
              event.preventDefault();
              const index = rail.findIndex((item) => item.id === activeRail);
              const delta = event.key === "ArrowDown" ? 1 : -1;
              const next = rail[(index + delta + rail.length) % rail.length];
              onRailChange(next.id);
              document
                .querySelector<HTMLElement>(`[data-testid='${railTestIdPrefix}-${next.id}']`)
                ?.focus();
            }}
          >
            {rail.map((item) => (
              <button
                key={item.id}
                type="button"
                role="tab"
                aria-selected={activeRail === item.id}
                tabIndex={activeRail === item.id ? 0 : -1}
                data-testid={`${railTestIdPrefix}-${item.id}`}
                data-dirty={item.dirty ? "true" : undefined}
                onClick={() => onRailChange(item.id)}
                className={`flex items-center justify-between gap-2 rounded px-3 py-2 text-left ${
                  activeRail === item.id ? "bg-bg-5 text-fg" : "text-fg-3 hover:bg-bg-4"
                }`}
              >
                <span>{item.label}</span>
                {item.dirty && (
                  <span
                    aria-label="Unsaved changes"
                    data-testid={`${railTestIdPrefix}-${item.id}-dirty`}
                    className="h-1.5 w-1.5 shrink-0 rounded-full bg-st-await"
                  />
                )}
              </button>
            ))}
          </nav>

          <div className="flex min-h-0 min-w-0 flex-1 flex-col">
            <main className={mainClassName}>{children}</main>
            {footer}
          </div>
        </div>

        {drawer}
      </div>
    </div>
  );
}
