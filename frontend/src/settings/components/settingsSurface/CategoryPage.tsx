import { useEffect, useLayoutEffect, useMemo, useRef, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { useScrollSpy } from "../../../hooks/useScrollSpy";
import {
  findCategory,
  type SettingsCategoryId,
  type SettingsSectionId,
} from "../../model/settingsCategories";
import { PageContext } from "./search";

/**
 * One category of the Settings surface: its scrollable page, and — drawn in the section
 * column of the surface (`navSlot`) while it is on screen — the list of its sections,
 * scroll-spied, each with its dirty dot. Under a search every page shows, one after the
 * other, in the surface's own scroll, and no section column.
 */
export default function CategoryPage({
  categoryId,
  active,
  searching = false,
  navSlot,
  ready,
  initialSection,
  highlight,
  dirtySections,
  onSectionChange,
  children,
}: {
  categoryId: SettingsCategoryId;
  active: boolean;
  /** The search line looks across every category. */
  searching?: boolean;
  /** Where the section column goes. */
  navSlot: HTMLElement | null;
  /**
   * `GET /settings` has answered (ok or not). Landing waits for it: a section that
   * renders on its own (Version & update, #697) exists before the ones above it, and a
   * scroll made then lands on a page the loading sections are about to push down.
   */
  ready: boolean;
  initialSection?: SettingsSectionId;
  /** Programmatic open (story 18): pulse the landed section so the eye finds it. */
  highlight?: boolean;
  dirtySections: ReadonlySet<SettingsSectionId>;
  onSectionChange: (section: SettingsSectionId) => void;
  children: ReactNode;
}) {
  const category = findCategory(categoryId);
  const sectionIds = useMemo(() => category.sections.map((s) => s.id), [category]);
  const scrollRef = useRef<HTMLDivElement>(null);
  const spying = active && !searching;
  const { active: activeSection, scrollTo } = useScrollSpy(sectionIds, scrollRef, spying);
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
    if (!spying || !ready || restored.current) return;
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
      // Pulse once (2.2 s, `settings.css`): only on a programmatic open, never on a click.
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
    if (spying) onSectionChange(activeSection);
  }, [spying, activeSection, onSectionChange]);

  const page = useMemo(
    () => ({ dirty: dirtySections as ReadonlySet<string>, category: category.label }),
    [dirtySections, category.label],
  );
  const shown = active || searching;

  return (
    <>
      {spying &&
        navSlot &&
        createPortal(
          <nav aria-label={`${category.label} sections`} data-testid="settings-subcolumn">
            {category.sections.map((section) => {
              const isActive = activeSection === section.id;
              const dirty = dirtySections.has(section.id);
              return (
                <button
                  key={section.id}
                  type="button"
                  onClick={() => scrollTo(section.id)}
                  aria-current={isActive ? "true" : undefined}
                  data-testid={`settings-section-${section.id}`}
                  data-dirty={dirty ? "true" : undefined}
                  className="set-nav__item"
                  title={section.label}
                >
                  <span className="set-nav__label">{section.label}</span>
                  {dirty && (
                    <span
                      role="img"
                      aria-label="Unsaved changes"
                      data-testid={`settings-section-${section.id}-dirty`}
                      className="set-dirty"
                    />
                  )}
                </button>
              );
            })}
          </nav>,
          navSlot,
        )}
      <div
        ref={scrollRef}
        hidden={!shown}
        className={shown ? "set-page" : undefined}
        data-testid={`settings-page-${categoryId}`}
      >
        <div className="set-page__col" data-testid={`settings-scroll-${categoryId}`}>
          <PageContext.Provider value={page}>{children}</PageContext.Provider>
        </div>
      </div>
    </>
  );
}
