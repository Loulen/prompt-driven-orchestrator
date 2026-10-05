import { useEffect, useRef, type ReactNode } from "react";
import { X } from "lucide-react";

export interface RailItem {
  id: string;
  label: string;
  /** Something under this entry deviates from its saved/default state —
   *  renders an amber dot (#690: unsaved edits; #810: Stats filters that differ
   *  from the defaults). */
  dirty?: boolean;
  /** What the dot means, for screen readers and the hover title. Defaults to
   *  Settings' « Unsaved changes » — Stats says why its numbers moved instead. */
  dirtyLabel?: string;
}

/**
 * The **panneau secondaire** (CONTEXT.md, #943): a panel the host describes and the shell
 * frames. It opens under the header — the title, the host's header actions and the ✕ stay
 * visible and clickable — and carries its own title, Escape hint and ✕. A click outside it
 * does not close it; Escape still goes through the host's `onEscape`. Once it closes, focus
 * returns to what opened it (the trigger) rather than falling to `<body>`.
 */
export interface ShellDrawer {
  title: string;
  /** Escape hint on the right of the panel header (« Esc returns to Settings »). */
  escHint: string;
  /** Panel header content, before the hint (Stats' « Sync costs »). */
  actions?: ReactNode;
  /** Width class of the panel, a literal Tailwind class (`w-[min(420px,90vw)]`). */
  widthClassName: string;
  /** `data-testid` of the panel; its ✕ is `${testId}-close` unless `closeTestId` says otherwise. */
  testId: string;
  closeTestId?: string;
  /** Extra `data-*` attributes on the panel (Settings' `data-drawer`). */
  dataAttributes?: Record<`data-${string}`, string>;
  /** Class of the scrolling body. */
  bodyClassName?: string;
  onClose: () => void;
  content: ReactNode;
}

interface Props {
  title: string;
  /** Drawn before the title (an extension page's icon). */
  titleIcon?: ReactNode;
  /** Rendered after the title, in the header row (Stats' period presets, etc.). */
  headerExtras?: ReactNode;
  /** Right-aligned header content, before the close button (hints, refresh…). */
  headerActions?: ReactNode;
  /** The left rail. Without one, the main slot spans the pane: an extension's
   *  `topbar/end` page brings its own navigation (#29). */
  rail?: RailItem[];
  activeRail?: string;
  onRailChange?: (id: string) => void;
  /** Rail width in px. Stats keeps 144; Settings needs 176 for "Sandbox & worktrees". */
  railWidth?: number;
  railAriaLabel?: string;
  /** `data-testid` prefix of a rail entry: `${prefix}-${id}`. */
  railTestIdPrefix?: string;
  /** The secondary panel, framed by the shell (Stats' pricing details, Settings' skill bank). */
  drawer?: ShellDrawer | null;
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
 * The full-window surface Stats, Settings and the extensions' pages share (#690): overlay,
 * header (title + ✕), optional left rail navigated with ↑↓, main slot, optional secondary panel
 * and optional footer.
 *
 * The shell owns Escape: ignored while a tooltip is open (the tooltip consumes it), else
 * delegated to `onEscape` (drawer first, then confirmation, then close — the host decides).
 * It listens on `window`, so a layer inside (an extension page's own dialog) that handles
 * Escape on `document` or on its elements, and calls `preventDefault()`, keeps the shell open.
 */
export default function FullWindowShell({
  title,
  titleIcon,
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
          <h2 className="flex items-center gap-2 font-semibold text-fg">
            {titleIcon}
            {title}
          </h2>
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
          {rail && (
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
                onRailChange?.(next.id);
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
                  onClick={() => onRailChange?.(item.id)}
                  className={`flex items-center justify-between gap-2 rounded px-3 py-2 text-left ${
                    activeRail === item.id ? "bg-bg-5 text-fg" : "text-fg-3 hover:bg-bg-4"
                  }`}
                >
                  <span>{item.label}</span>
                  {item.dirty && (
                    <span
                      aria-label={item.dirtyLabel ?? "Unsaved changes"}
                      title={item.dirtyLabel ?? "Unsaved changes"}
                      data-testid={`${railTestIdPrefix}-${item.id}-dirty`}
                      className="h-1.5 w-1.5 shrink-0 rounded-full bg-st-await"
                    />
                  )}
                </button>
              ))}
            </nav>
          )}

          <div className="flex min-h-0 min-w-0 flex-1 flex-col">
            <main className={mainClassName}>{children}</main>
            {footer}
          </div>
        </div>

        {drawer && <SecondaryPanel drawer={drawer} />}
      </div>
    </div>
  );
}

/** The frame of the secondary panel: under the header, title + actions + Escape hint + ✕. */
function SecondaryPanel({ drawer }: { drawer: ShellDrawer }) {
  const panelRef = useRef<HTMLElement>(null);
  // Focus return (#944): the element focused when the panel mounted is its opener (the
  // trigger clicked or activated from the keyboard). When the panel goes — ✕, Escape or the
  // toggle — focus goes back to it, unless the user has already moved it elsewhere or the
  // opener is gone (tab change, programmatic entry from another surface).
  useEffect(() => {
    const panel = panelRef.current;
    const opener = document.activeElement;
    return () => {
      if (!(opener instanceof HTMLElement) || !opener.isConnected) return;
      if (panel?.contains(opener)) return;
      const active = document.activeElement;
      if (active && active !== document.body && !panel?.contains(active)) return;
      opener.focus();
    };
  }, []);

  return (
    <aside
      ref={panelRef}
      className={`absolute bottom-0 right-0 top-14 z-20 flex flex-col border-l border-line bg-bg-4 shadow-2xl ${drawer.widthClassName}`}
      aria-label={drawer.title}
      data-testid={drawer.testId}
      {...drawer.dataAttributes}
    >
      <div className="flex items-center gap-3 border-b border-line px-4 py-3">
        <h3 className="font-semibold text-fg" style={{ fontSize: "13px" }}>
          {drawer.title}
        </h3>
        <div className="ml-auto flex items-center gap-3">
          {drawer.actions}
          <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
            {drawer.escHint}
          </span>
        </div>
        <button
          type="button"
          onClick={drawer.onClose}
          aria-label="Close panel"
          data-testid={drawer.closeTestId ?? `${drawer.testId}-close`}
          className="grid h-6 w-6 shrink-0 place-items-center rounded text-fg-3 transition-colors hover:bg-bg-5 hover:text-fg"
        >
          <X size={14} />
        </button>
      </div>
      <div className={drawer.bodyClassName ?? "flex min-h-0 flex-1 flex-col overflow-y-auto"}>
        {drawer.content}
      </div>
    </aside>
  );
}
