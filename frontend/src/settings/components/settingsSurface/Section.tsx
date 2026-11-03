import { useContext, type ReactNode } from "react";
import type { SettingsSection } from "../../model/settingsCategories";
import { ScopeIcon } from "./fields";
import { matches, PageContext, searching, SearchContext, SectionMatchContext } from "./search";

/**
 * One section of a category page: a title bar (the ruby square, the title, the scope
 * icon, read-only, the dirty dot, tools on the right) over its rows. Under a search, the
 * category's name comes first, and a section whose name is not found shows its rows only.
 */
export default function Section({
  section,
  badge,
  tools,
  children,
}: {
  /** A core section, or an extension's (keyed by its contribution). */
  section: Pick<SettingsSection, "label" | "readOnly" | "ownPersistence" | "deviceLocal" | "account"> & { id: string };
  /** After the title (who added an extension's section). */
  badge?: ReactNode;
  /** Icon buttons at the right of the title bar. */
  tools?: ReactNode;
  children: ReactNode;
}) {
  const page = useContext(PageContext);
  const query = useContext(SearchContext);
  const inSearch = searching(query);
  const labelMatch = inSearch && !query.modifiedOnly && matches(query.needle, section.label);
  const scope = section.account ? "account" : section.deviceLocal ? "device" : section.ownPersistence ? "live" : undefined;
  return (
    <section
      id={`settings-section-${section.id}`}
      data-section-id={section.id}
      data-testid={`settings-section-body-${section.id}`}
      data-label-match={labelMatch ? "true" : undefined}
      className="set-sec"
    >
      <header className="set-sec__head">
        {inSearch && page.category && <span className="set-sec__cat">{page.category} ›</span>}
        <h3 className="set-sec__title">{section.label}</h3>
        {scope && <ScopeIcon scope={scope} testId={`settings-section-${section.id}-scope`} />}
        {section.readOnly && <span className="d-chip">read-only</span>}
        {badge}
        {page.dirty.has(section.id) && (
          <span className="set-dirty" role="img" title="Unsaved changes" aria-label="Unsaved changes" />
        )}
        {tools && <span className="set-sec__tools">{tools}</span>}
      </header>
      <SectionMatchContext.Provider value={labelMatch}>
        <div className="set-sec__body" data-filtered={inSearch && !labelMatch ? "true" : undefined}>
          {children}
        </div>
      </SectionMatchContext.Provider>
    </section>
  );
}

/**
 * The content of a section that is not a setting row (a table, a panel): hidden under a
 * search that did not find the section, yet kept mounted, so a panel keeps its draft and
 * reads nothing again when the search is cleared.
 */
export function SectionExtra({ children, testId }: { children: ReactNode; testId?: string }) {
  const query = useContext(SearchContext);
  const labelMatch = useContext(SectionMatchContext);
  return (
    <div className="set-extra" hidden={searching(query) && !labelMatch} data-testid={testId}>
      {children}
    </div>
  );
}
