import { useEffect, useLayoutEffect, useMemo, useRef, type ReactNode } from "react";
import { useScrollSpy } from "../../../hooks/useScrollSpy";
import {
  findCategory,
  type SettingsCategoryId,
  type SettingsSectionId,
} from "../../model/settingsCategories";

/**
 * One category of the Settings surface: its sub-column listing the sections (scroll-spied,
 * each with its dirty dot) and its scrollable page.
 */
export default function CategoryPage({
  categoryId,
  active,
  ready,
  initialSection,
  highlight,
  dirtySections,
  onSectionChange,
  children,
}: {
  categoryId: SettingsCategoryId;
  active: boolean;
  /**
   * `GET /settings` has answered (ok or not). Landing waits for it: a section that
   * renders on its own (Version & update, #697) exists before the ones above it, and a
   * scroll made then lands on a page the loading sections are about to push down.
   */
  ready: boolean;
  initialSection?: SettingsSectionId;
  /** Programmatic open (story 18): pulse the landed section so the eye finds it. */
  highlight?: boolean;
  dirtySections: Set<SettingsSectionId>;
  onSectionChange: (section: SettingsSectionId) => void;
  children: ReactNode;
}) {
  const category = findCategory(categoryId);
  const sectionIds = useMemo(() => category.sections.map((s) => s.id), [category]);
  const scrollRef = useRef<HTMLDivElement>(null);
  const { active: activeSection, scrollTo } = useScrollSpy(sectionIds, scrollRef, active);
  const restored = useRef(false);

  // Land on the remembered / requested section (stories 17 and 18) — once, instantly,
  // before paint. No dependency list on purpose: the sections mount after `GET /settings`
  // resolves, so the effect re-checks on each render until the target exists, then stops.
  // The target is captured once: while the page still shows "loading", the spy reports the
  // first section and the parent remembers it, which would silently replace the request.
  // `ready` gates the whole thing: the target may exist before its predecessors do.
  const target = useRef(initialSection);
  const landedTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useLayoutEffect(() => {
    if (!active || !ready || restored.current) return;
    const wanted = target.current;
    if (!wanted || wanted === sectionIds[0]) {
      restored.current = true;
      return;
    }
    const el = scrollRef.current?.querySelector<HTMLElement>(`[data-section-id="${wanted}"]`);
    if (!el) return;
    restored.current = true;
    // Instant, not smooth: the panels below still grow while they load, and a smooth scroll
    // racing that growth ends short. The spy re-picks on the content's ResizeObserver.
    el.scrollIntoView?.({ behavior: "auto", block: "start" });
    scrollTo(wanted);
    if (highlight) {
      // Pulse once (2.2 s, `index.css`): only on a programmatic open, never on a click.
      el.dataset.landed = "true";
      landedTimer.current = setTimeout(() => {
        delete el.dataset.landed;
      }, 2200);
    }
  });
  useEffect(
    () => () => {
      if (landedTimer.current) clearTimeout(landedTimer.current);
    },
    [],
  );

  useEffect(() => {
    if (active) onSectionChange(activeSection);
  }, [active, activeSection, onSectionChange]);

  return (
    <div
      hidden={!active}
      className={active ? "flex min-h-0 min-w-0 flex-1" : undefined}
      data-testid={`settings-page-${categoryId}`}
    >
      <nav
        className="w-[168px] shrink-0 border-r border-line px-3 py-4"
        aria-label={`${category.label} sections`}
        data-testid="settings-subcolumn"
      >
        <div
          className="mb-2 px-2 uppercase tracking-wider text-fg-4"
          style={{ fontSize: "10px" }}
        >
          {category.label}
        </div>
        <ul className="flex flex-col gap-0.5">
          {category.sections.map((section) => {
            const isActive = activeSection === section.id;
            const dirty = dirtySections.has(section.id);
            return (
              <li key={section.id}>
                <button
                  type="button"
                  onClick={() => scrollTo(section.id)}
                  aria-current={isActive ? "true" : undefined}
                  data-testid={`settings-section-${section.id}`}
                  data-dirty={dirty ? "true" : undefined}
                  className={`flex w-full items-center justify-between gap-2 border-l-2 px-2 py-1 text-left ${
                    isActive
                      ? "border-acc bg-acc-bg text-fg"
                      : "border-transparent text-fg-3 hover:text-fg-2"
                  }`}
                  style={{ fontSize: "11.5px" }}
                >
                  <span>{section.label}</span>
                  {dirty && (
                    <span
                      aria-label="Unsaved changes"
                      data-testid={`settings-section-${section.id}-dirty`}
                      className="h-1.5 w-1.5 shrink-0 rounded-full bg-st-await"
                    />
                  )}
                </button>
              </li>
            );
          })}
        </ul>
      </nav>
      <div
        ref={scrollRef}
        className="min-w-0 flex-1 overflow-y-auto px-7 py-5"
        data-testid={`settings-scroll-${categoryId}`}
      >
        <div className="flex max-w-[640px] flex-col gap-7 pb-10">{children}</div>
      </div>
    </div>
  );
}
